//! Lenient parsing of theme TOML tables.

use std::collections::BTreeMap;

use toml::{Table, Value};

use super::{Appearance, Color, SyntaxStyle, TerminalColors, Theme, ThemeError, UiColors};
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

    let name = match table.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => name.trim().to_owned(),
        _ => fallback_name
            .map(str::to_owned)
            .ok_or_else(|| ThemeError("missing `name`".into()))?,
    };
    let appearance = appearance(table, base, &mut warnings)?;

    let ui_map = read_colors(table.get("ui"), "ui", UiColors::FIELDS, &mut warnings);
    let ui = UiColors::from_lookup(|key| {
        ui_map
            .get(key)
            .copied()
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
        let path = format!("syntax.\"{capture}\"");
        match syntax_style(value, &path, warnings) {
            Ok(style) => {
                syntax.insert(capture.clone(), style);
            }
            Err(err) => warnings.push(format!("`{path}`: {err}; ignored")),
        }
    }
}

fn syntax_style(
    value: &Value,
    path: &str,
    warnings: &mut Vec<String>,
) -> Result<SyntaxStyle, String> {
    let table = match value {
        Value::String(_) => {
            return Ok(SyntaxStyle {
                color: Some(color_value(value)?),
                ..SyntaxStyle::default()
            });
        }
        Value::Table(table) => table,
        other => {
            return Err(format!(
                "expected a color string or a table, found {}",
                other.type_str()
            ));
        }
    };
    let mut style = SyntaxStyle::default();
    for (key, value) in table {
        let flag = match key.as_str() {
            "color" => {
                style.color = Some(color_value(value)?);
                continue;
            }
            "bold" => &mut style.bold,
            "italic" => &mut style.italic,
            "underline" => &mut style.underline,
            _ => {
                warnings.push(format!("unknown key `{path}.{key}` (ignored)"));
                continue;
            }
        };
        *flag = value
            .as_bool()
            .ok_or_else(|| format!("`{key}` must be true or false"))?;
    }
    Ok(style)
}
