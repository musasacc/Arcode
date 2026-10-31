//! Lenient parsing of theme TOML tables.

use std::collections::BTreeMap;

use toml::{Table, Value};

use super::{Appearance, Color, SyntaxStyle, TerminalColors, Theme, ThemeError, UiColors, derive};
use crate::lenient::one_line;

const TOP_LEVEL_KEYS: [&str; 6] = ["name", "appearance", "extends", "ui", "syntax", "terminal"];

/// Parses TOML source into a table, mapping syntax errors to [`ThemeError`].
pub(super) fn parse_table(source: &str) -> Result<Table, ThemeError> {
    toml::from_str::<Table>(source)
        .map_err(|err| ThemeError(format!("not valid TOML: {}", one_line(&err.to_string()))))
}

pub(super) fn theme_from_table(
    table: &Table,
    base: Option<&Theme>,
    fallback_name: Option<&str>,
) -> Result<(Theme, Vec<String>), ThemeError> {
    let mut warnings = Vec::new();
    for key in table.keys() {
        if !TOP_LEVEL_KEYS.contains(&key.as_str()) {
            warnings.push(format!("unknown key `{key}` (ignored)"));
        }
    }

    let name =
        theme_name(table, fallback_name).ok_or_else(|| ThemeError("missing `name`".into()))?;
    let appearance = appearance(table, base, &mut warnings)?;

    let ui_map = read_colors(table.get("ui"), "ui", UiColors::FIELDS, &mut warnings);
    let resolved = |key: &str| {
        ui_map
            .get(key)
            .copied()
            .or_else(|| base.and_then(|b| b.ui.get(key)))
    };
    // A key the table sets wins; a newer key the table omits is derived
    // from the table's older keys when it sets those; else it is inherited.
    let ui = UiColors::from_lookup(|key| {
        ui_map
            .get(key)
            .copied()
            .or_else(|| derive::derive(key, |k| ui_map.contains_key(k), resolved))
            .or_else(|| base.and_then(|b| b.ui.get(key)))
    })
    .map_err(|key| ThemeError(format!("missing ui color `{key}`")))?;

    let term_map = read_colors(
        table.get("terminal"),
        "terminal",
        &TerminalColors::FIELDS,
        &mut warnings,
    );
    let terminal = TerminalColors::from_lookup(|key| {
        term_map
            .get(key)
            .copied()
            .or_else(|| base.and_then(|b| b.terminal.get(key)))
    })
    .map_err(|key| ThemeError(format!("missing terminal color `{key}`")))?;

    let mut syntax = base.map(|b| b.syntax.clone()).unwrap_or_default();
    read_syntax(table.get("syntax"), &mut syntax, &mut warnings);

    let theme = Theme {
        name,
        appearance,
        ui,
        syntax,
        terminal,
    };
    Ok((theme, warnings))
}

/// The name a theme table will get: its trimmed `name`, else `fallback_name`.
pub(super) fn theme_name(table: &Table, fallback_name: Option<&str>) -> Option<String> {
    match table.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => Some(name.trim().to_owned()),
        _ => fallback_name.map(str::to_owned),
    }
}

fn appearance(
    table: &Table,
    base: Option<&Theme>,
    warnings: &mut Vec<String>,
) -> Result<Appearance, ThemeError> {
    let inherited = base.map(|b| b.appearance);
    match table.get("appearance") {
        None => inherited.ok_or_else(|| ThemeError("missing `appearance`".into())),
        Some(value) => match value.as_str().and_then(Appearance::parse) {
            Some(appearance) => Ok(appearance),
            None => {
                let message =
                    format!("invalid `appearance` {value} (expected \"dark\" or \"light\")");
                match inherited {
                    Some(appearance) => {
                        warnings.push(format!("{message}; using {appearance:?}"));
                        Ok(appearance)
                    }
                    None => Err(ThemeError(message)),
                }
            }
        },
    }
}

/// Parses a color value, describing the problem on failure.
pub(super) fn color_value(value: &Value) -> Result<Color, String> {
    match value.as_str() {
        Some(text) => text.parse().map_err(|err| format!("{err}")),
        None => Err(format!(
            "expected a color string, found {}",
            value.type_str()
        )),
    }
}

fn read_colors(
    section: Option<&Value>,
    path: &str,
    known: &[&str],
    warnings: &mut Vec<String>,
) -> BTreeMap<String, Color> {
    let mut colors = BTreeMap::new();
    let Some(section) = section else {
        return colors;
    };
    let Some(table) = section.as_table() else {
        warnings.push(format!("`{path}` must be a table (ignored)"));
        return colors;
    };
    for (key, value) in table {
        if !known.contains(&key.as_str()) {
            warnings.push(format!("unknown key `{path}.{key}` (ignored)"));
            continue;
        }
        match color_value(value) {
            Ok(color) => {
                colors.insert(key.clone(), color);
            }
            Err(err) => warnings.push(format!("`{path}.{key}`: {err}; inherited")),
        }
    }
    colors
}

/// Keys of a syntax style table; any other key in a `[syntax]` table names
/// a nested capture (`function.method = ".."` parses as `function = { method = ".." }`).
const STYLE_KEYS: [&str; 4] = ["color", "bold", "italic", "underline"];

fn read_syntax(
    section: Option<&Value>,
    syntax: &mut BTreeMap<String, SyntaxStyle>,
    warnings: &mut Vec<String>,
) {
    let Some(section) = section else {
        return;
    };
    let Some(table) = section.as_table() else {
        warnings.push("`syntax` must be a table (ignored)".into());
        return;
    };
    for (capture, value) in table {
        read_syntax_entry(capture, value, syntax, warnings);
    }
}

/// Reads the style of `capture` and, for tables, of its nested captures.
///
/// A string is a color. A table may mix style keys (which style `capture`
/// itself, replacing any inherited style) with nested captures; a table of
/// only nested captures leaves the inherited style of `capture` untouched.
fn read_syntax_entry(
    capture: &str,
    value: &Value,
    syntax: &mut BTreeMap<String, SyntaxStyle>,
    warnings: &mut Vec<String>,
) {
    let path = format!("syntax.\"{capture}\"");
    let table = match value {
        Value::String(_) => {
            match color_value(value) {
                Ok(color) => {
                    let style = SyntaxStyle {
                        color: Some(color),
                        ..SyntaxStyle::default()
                    };
                    syntax.insert(capture.to_owned(), style);
                }
                Err(err) => warnings.push(format!("`{path}`: {err}; ignored")),
            }
            return;
        }
        Value::Table(table) => table,
        other => {
            warnings.push(format!(
                "`{path}`: expected a color string or a style table \
                 (color, bold, italic, underline), found {}; ignored",
                other.type_str()
            ));
            return;
        }
    };
    let mut own = Table::new();
    for (key, value) in table {
        if STYLE_KEYS.contains(&key.as_str()) {
            own.insert(key.clone(), value.clone());
        } else {
            read_syntax_entry(&format!("{capture}.{key}"), value, syntax, warnings);
        }
    }
    if own.is_empty() {
        return;
    }
    match style_from_table(&own) {
        Ok(style) => {
            syntax.insert(capture.to_owned(), style);
        }
        Err(err) => warnings.push(format!("`{path}`: {err}; ignored")),
    }
}

/// Builds a style from a table holding only [`STYLE_KEYS`].
fn style_from_table(table: &Table) -> Result<SyntaxStyle, String> {
    let mut style = SyntaxStyle::default();
    for (key, value) in table {
        let flag = match key.as_str() {
            "color" => {
                style.color = Some(color_value(value)?);
                continue;
            }
            "bold" => &mut style.bold,
            "italic" => &mut style.italic,
            _ => &mut style.underline,
        };
        *flag = value
            .as_bool()
            .ok_or_else(|| format!("`{key}` must be true or false"))?;
    }
    Ok(style)
}
