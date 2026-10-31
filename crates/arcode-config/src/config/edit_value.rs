//! Setting single values of `config.toml` from the Settings page, keeping
//! the rest of the file (comments, layout, other keys) as it was.

use toml_edit::{DocumentMut, Item, Value};

use super::edit::{parse, table_entry};

/// A value the Settings page writes.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    /// `true` / `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float, rounded to three decimals when written.
    Float(f64),
    /// A string.
    Str(String),
}

impl SettingValue {
    fn to_toml(&self) -> Value {
        match self {
            Self::Bool(b) => Value::from(*b),
            Self::Int(i) => Value::from(*i),
            Self::Float(f) => Value::from((f * 1000.).round() / 1000.),
            Self::Str(s) => Value::from(s.as_str()),
        }
    }
}

impl SettingValue {
    /// The value of a TOML value of the same kind.
    fn matches(&self, value: &Value) -> bool {
        match self {
            Self::Bool(b) => value.as_bool() == Some(*b),
            Self::Int(i) => value.as_integer() == Some(*i),
            Self::Float(f) => value.as_float() == Some(*f),
            Self::Str(s) => value.as_str() == Some(s.as_str()),
        }
    }
}

impl From<bool> for SettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f32> for SettingValue {
    fn from(value: f32) -> Self {
        // Through the shortest decimal form, so 1.6f32 is written as 1.6.
        Self::Float(value.to_string().parse().unwrap_or(value as f64))
    }
}

impl From<u64> for SettingValue {
    fn from(value: u64) -> Self {
        Self::Int(value.min(i64::MAX as u64) as i64)
    }
}

impl From<usize> for SettingValue {
    fn from(value: usize) -> Self {
        Self::from(value as u64)
    }
}

impl From<u32> for SettingValue {
    fn from(value: u32) -> Self {
        Self::Int(value.into())
    }
}

impl From<&str> for SettingValue {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for SettingValue {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

/// The comment a preset (low-end mode) leaves on a value it changed, so
/// turning the preset off restores only what the preset changed.
pub const PRESET_MARK: &str = "[low-end mode]";

/// When a [`SettingEdit`] applies.
#[derive(Clone, Debug, PartialEq)]
pub enum Guard {
    /// Only when the key is set to this value now.
    Is(SettingValue),
    /// Only when the key is set to this value now; the new value is tagged
    /// with [`PRESET_MARK`].
    IsThenMark(SettingValue),
    /// Only when the key's value carries [`PRESET_MARK`]; the tag is
    /// removed.
    Marked,
}

/// One change: set `path` (dotted, e.g. `editor.font_size`) to a value, or
/// remove the key (`None`) so the built-in default applies again.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingEdit {
    /// Dotted key path; every part but the last names a table.
    pub path: String,
    /// The new value, or `None` to remove the key.
    pub value: Option<SettingValue>,
    /// Applies only under this condition (`None`: always).
    pub guard: Option<Guard>,
}

impl SettingEdit {
    /// Sets `path` to `value`.
    pub fn set(path: impl Into<String>, value: impl Into<SettingValue>) -> Self {
        Self {
            path: path.into(),
            value: Some(value.into()),
            guard: None,
        }
    }

    /// Removes `path`.
    pub fn remove(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            value: None,
            guard: None,
        }
    }

    /// The same edit, applied only under `guard`.
    pub fn when(mut self, guard: Guard) -> Self {
        self.guard = Some(guard);
        self
    }
}

/// Whether `guard` lets an edit replace `old`.
fn guard_allows(guard: &Guard, old: Option<&Value>) -> bool {
    let Some(old) = old else {
        return false;
    };
    match guard {
        Guard::Is(value) | Guard::IsThenMark(value) => value.matches(old),
        Guard::Marked => old
            .decor()
            .suffix()
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.contains(PRESET_MARK)),
    }
}

/// The trailing comment of a replaced value, with the preset tag added
/// (`mark`) or removed.
fn marked_suffix(old: Option<&str>, mark: bool) -> String {
    let old = old.unwrap_or("").trim_end();
    if mark {
        if old.trim().is_empty() {
            format!(" # {PRESET_MARK}")
        } else {
            format!("{old} {PRESET_MARK}")
        }
    } else {
        let stripped = old
            .replace(&format!(" {PRESET_MARK}"), "")
            .replace(PRESET_MARK, "");
        if stripped.trim().trim_start_matches('#').trim().is_empty() {
            String::new()
        } else {
            stripped
        }
    }
}

/// Applies `edits` in order. A replaced value keeps its trailing comment;
/// a new key is appended to its table (created when missing). Fails only
/// when `source` is not valid TOML or a path crosses a non-table.
pub fn apply_edits(source: &str, edits: &[SettingEdit]) -> Result<String, String> {
    let mut doc: DocumentMut = parse(source)?;
    for edit in edits {
        let parts: Vec<&str> = edit.path.split('.').collect();
        let Some((key, tables)) = parts.split_last() else {
            continue;
        };
        if key.is_empty() {
            return Err(format!("`{}` is not a key", edit.path));
        }
        if edit.value.is_none() && !has_tables(&doc, tables) {
            // Nothing to remove.
            continue;
        }
        let mut table = doc.as_table_mut() as &mut dyn toml_edit::TableLike;
        for name in tables {
            table = table_entry(table, name)?;
        }
        let old = table.get(key).and_then(Item::as_value);
        if let Some(guard) = &edit.guard
            && !guard_allows(guard, old)
        {
            continue;
        }
        match &edit.value {
            Some(value) => {
                let mut new = value.to_toml();
                if let Some(old) = old {
                    *new.decor_mut() = old.decor().clone();
                    let mark = match edit.guard {
                        Some(Guard::IsThenMark(_)) => Some(true),
                        Some(Guard::Marked) => Some(false),
                        _ => None,
                    };
                    if let Some(mark) = mark {
                        let suffix = old.decor().suffix().and_then(|s| s.as_str());
                        new.decor_mut().set_suffix(marked_suffix(suffix, mark));
                    }
                }
                match table.get_mut(key) {
                    Some(item) => *item = Item::Value(new),
                    None => {
                        table.insert(key, Item::Value(new));
                    }
                }
            }
            None => {
                table.remove(key);
            }
        }
    }
    Ok(doc.to_string())
}

/// Whether every table along `tables` exists.
fn has_tables(doc: &DocumentMut, tables: &[&str]) -> bool {
    let mut table: &dyn toml_edit::TableLike = doc.as_table();
    for name in tables {
        match table.get(name).and_then(Item::as_table_like) {
            Some(next) => table = next,
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, ThemeChoice};

    const SOURCE: &str = "# mine\n[editor]\nfont_size = 15.0 # big\ntab_size = 2\n\n[ui]\n# font_family = \"Inter\"\ntheme = \"dark\"\n";

    #[test]
    fn replacing_keeps_comments_and_other_keys() {
        let out = apply_edits(
            SOURCE,
            &[
                SettingEdit::set("editor.font_size", 16.5f32),
                SettingEdit::set("ui.theme", "Arcode Midnight"),
            ],
        )
        .unwrap();
        assert!(out.contains("font_size = 16.5 # big"), "{out}");
        assert!(out.contains("tab_size = 2"));
        assert!(out.contains("# font_family = \"Inter\""));
        assert!(out.starts_with("# mine\n"));
        let (config, warnings) = Config::from_toml_str(&out);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.editor.font_size, 16.5);
        assert_eq!(
            config.ui.theme,
            ThemeChoice::Named("Arcode Midnight".into())
        );
    }

    #[test]
    fn new_keys_and_tables_are_added() {
        let out = apply_edits(
            SOURCE,
            &[
                SettingEdit::set("ui.font_family", "Inter"),
                SettingEdit::set("git.inline_blame", false),
                SettingEdit::set("terminal.scrollback", 5000u64),
            ],
        )
        .unwrap();
        let (config, warnings) = Config::from_toml_str(&out);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.ui.font_family.as_deref(), Some("Inter"));
        assert!(!config.git.inline_blame);
        assert_eq!(config.terminal.scrollback, 5000);
    }

    #[test]
    fn removing_restores_the_default() {
        let out = apply_edits(
            SOURCE,
            &[
                SettingEdit::remove("editor.tab_size"),
                SettingEdit::remove("search.missing"),
            ],
        )
        .unwrap();
        assert!(!out.contains("tab_size"));
        assert!(!out.contains("[search]"), "no table is created to remove");
        assert_eq!(Config::from_toml_str(&out).0.editor.tab_size, 4);
    }

    #[test]
    fn guarded_edits_apply_only_when_their_condition_holds() {
        let source = "[editor]\nhints = true # mine\nblame = false\n";
        let mark = |key: &str| SettingEdit::set(key, false).when(Guard::IsThenMark(true.into()));
        let on = apply_edits(
            source,
            &[mark("editor.hints"), mark("editor.blame"), mark("editor.x")],
        )
        .unwrap();
        assert!(on.contains("hints = false # mine [low-end mode]\n"), "{on}");
        assert!(on.contains("blame = false\n"), "{on}");
        assert!(!on.contains("x ="), "a missing key is left unset: {on}");
        let unmark = |key: &str| SettingEdit::set(key, true).when(Guard::Marked);
        let off = apply_edits(&on, &[unmark("editor.hints"), unmark("editor.blame")]).unwrap();
        assert_eq!(off, source);
        let plain = apply_edits("[a]\nb = true\n", &[mark("a.b")]).unwrap();
        assert_eq!(plain, "[a]\nb = false # [low-end mode]\n");
        let back = apply_edits(&plain, &[unmark("a.b")]).unwrap();
        assert_eq!(back, "[a]\nb = true\n");
        let is = SettingEdit::set("a.b", 1u64).when(Guard::Is(false.into()));
        assert_eq!(
            apply_edits("[a]\nb = true\n", &[is]).unwrap(),
            "[a]\nb = true\n"
        );
    }

    #[test]
    fn floats_are_written_short() {
        let out = apply_edits("", &[SettingEdit::set("editor.line_height", 1.6f32)]).unwrap();
        assert!(out.contains("line_height = 1.6\n"), "{out}");
    }

    #[test]
    fn invalid_sources_and_paths_are_refused() {
        assert!(apply_edits("[editor", &[SettingEdit::set("a.b", true)]).is_err());
        assert!(apply_edits("editor = 3\n", &[SettingEdit::set("editor.x", true)]).is_err());
        assert!(apply_edits("", &[SettingEdit::set("editor.", true)]).is_err());
    }
}
