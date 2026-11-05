//! Single settings by their dotted key (`editor.tab_size`): the value a
//! configuration has, the built-in default, and the edit that puts a key
//! back to its default. The Settings page uses them to mark changed rows
//! and to reset them.

use std::sync::OnceLock;

use toml::Value;

use super::Config;
use super::edit_value::{Guard, SettingEdit, SettingValue};
use super::presets::LOW_END_OFF;

/// Keys whose default changes while low-end mode is on.
const LOW_END_IMPLIED: [&str; 4] = [
    "editor.cursor_blink",
    "editor.minimap",
    "editor.inlay_hints",
    "git.inline_blame",
];

/// The value of `path` in `config` as TOML; `None` for an unset optional
/// setting, a key that does not exist, or the per-language and per-server
/// tables.
pub fn setting_value(config: &Config, path: &str) -> Option<Value> {
    let (section, rest) = path.split_once('.')?;
    let table = match section {
        "editor" => Value::try_from(&config.editor),
        "ui" => Value::try_from(&config.ui),
        "terminal" => Value::try_from(&config.terminal),
        "git" => Value::try_from(&config.git),
        "search" => Value::try_from(&config.search),
        "todo" => Value::try_from(&config.todo),
        "workspace" => Value::try_from(&config.workspace),
        "debug" => Value::try_from(&config.debug),
        "files" => Value::try_from(&config.files),
        "plugins" => Value::try_from(&config.plugins),
        _ => return None,
    }
    .ok()?;
    let mut value = &table;
    for key in rest.split('.') {
        value = value.get(key)?;
    }
    Some(value.clone())
}

/// The default of `path`. For the settings low-end mode turns off, it is
/// their low-end value while `low_end` is on.
pub fn default_value(path: &str, low_end: bool) -> Option<Value> {
    static DEFAULTS: OnceLock<(Config, Config)> = OnceLock::new();
    let (normal, low) = DEFAULTS.get_or_init(|| {
        let low = Config::from_toml_str("[ui]\nlow_end_mode = true\n").0;
        (Config::default(), low)
    });
    let config = if low_end && LOW_END_IMPLIED.contains(&path) {
        low
    } else {
        normal
    };
    setting_value(config, path)
}

/// Whether `config` has the default value for `path`.
pub fn is_default(config: &Config, path: &str) -> bool {
    setting_value(config, path) == default_value(path, config.ui.low_end_mode)
}

/// The edit that puts `path` back to its default. The default value is
/// written in place, so the key keeps its comments; an optional setting
/// that is unset by default is removed. While low-end mode is on, inlay
/// hints and inline blame go back to off with the low-end tag, so turning
/// low-end mode off turns them on again.
pub fn reset_edit(path: &str, low_end: bool) -> SettingEdit {
    if low_end && LOW_END_OFF.contains(&path) {
        return SettingEdit::set(path, false).when(Guard::IsThenMark(true.into()));
    }
    match default_value(path, false).as_ref().and_then(from_toml) {
        Some(value) => SettingEdit::set(path, value),
        None => SettingEdit::remove(path),
    }
}

/// The default of `path` in words, for a reset button's tooltip: "on",
/// "14", "JetBrains Mono", "4 MB", "empty", "unset".
pub fn shown_default(path: &str, low_end: bool) -> String {
    const MIB: i64 = 1024 * 1024;
    match default_value(path, low_end) {
        None => "unset".into(),
        Some(Value::Boolean(on)) => if on { "on" } else { "off" }.into(),
        Some(Value::Integer(bytes)) if path.ends_with("_bytes") && bytes % MIB == 0 => {
            format!("{} MB", bytes / MIB)
        }
        Some(Value::Float(f)) => format!("{}", f as f32),
        Some(Value::String(s)) if s.is_empty() => "empty".into(),
        Some(Value::String(s)) => s,
        Some(Value::Array(items)) if items.is_empty() => "empty".into(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    }
}

/// The written form of a TOML value, where the Settings page can write it.
fn from_toml(value: &Value) -> Option<SettingValue> {
    Some(match value {
        Value::Boolean(b) => SettingValue::Bool(*b),
        Value::Integer(i) => SettingValue::Int(*i),
        // Through f32, the type of every float setting: 1.8, not 1.79999…
        Value::Float(f) => SettingValue::from(*f as f32),
        Value::String(s) => SettingValue::Str(s.clone()),
        Value::Array(items) => SettingValue::List(
            items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
        ),
        _ => return None,
    })
}

/// Every key of the fixed sections (`editor.tab_size`,
/// `editor.completion.delay_ms`, ...), in the order of
/// [`super::known_keys`]; the per-language and per-server tables are left
/// out.
pub fn documented_keys() -> Vec<String> {
    super::known_keys()
        .into_iter()
        .filter(|(section, _)| !section.contains('<'))
        .flat_map(|(section, keys)| keys.iter().map(move |key| format!("{section}.{key}")))
        // `completion` is the `[editor.completion]` table, listed by itself.
        .filter(|key| key != "editor.completion")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_edits;

    #[test]
    fn values_are_looked_up_by_key() {
        let config = Config::default();
        assert_eq!(
            setting_value(&config, "editor.tab_size"),
            Some(Value::Integer(4))
        );
        assert_eq!(
            setting_value(&config, "editor.completion.delay_ms"),
            Some(Value::Integer(50))
        );
        assert_eq!(
            setting_value(&config, "ui.theme"),
            Some(Value::String("system".into()))
        );
        assert_eq!(setting_value(&config, "terminal.shell"), None);
        assert_eq!(setting_value(&config, "editor.nope"), None);
        assert_eq!(setting_value(&config, "language.rust.enabled"), None);
        for key in documented_keys() {
            let optional = [
                "ui.font_family",
                "terminal.shell",
                "terminal.font_family",
                "terminal.font_size",
                "workspace.projects_folder",
            ];
            assert_eq!(
                setting_value(&config, &key).is_some(),
                !optional.contains(&key.as_str()),
                "{key}"
            );
            assert!(is_default(&config, &key), "{key}");
        }
    }

    #[test]
    fn changed_values_are_not_default_and_reset_writes_the_default() {
        let source = "[editor]\n# Width of a tab stop.\ntab_size = 2 # mine\nline_height = 1.8\n";
        let (config, _) = Config::from_toml_str(source);
        assert!(!is_default(&config, "editor.tab_size"));
        assert!(!is_default(&config, "editor.line_height"));
        assert!(is_default(&config, "editor.font_size"));
        let out = apply_edits(
            source,
            &[
                reset_edit("editor.tab_size", false),
                reset_edit("editor.line_height", false),
            ],
        )
        .unwrap();
        assert_eq!(
            out,
            "[editor]\n# Width of a tab stop.\ntab_size = 4 # mine\nline_height = 1.5\n"
        );
        assert_eq!(
            reset_edit("search.exclude", false),
            SettingEdit::set("search.exclude", SettingValue::List(Vec::new()))
        );
        assert_eq!(
            reset_edit("ui.text_gamma", false),
            SettingEdit::set("ui.text_gamma", 1.8f32)
        );
    }

    #[test]
    fn unset_by_default_is_removed_keeping_the_comment_above() {
        let source =
            "[terminal]\n# Shell program.\nshell = \"/bin/zsh\"\n# Arguments.\nargs = []\n";
        assert_eq!(
            reset_edit("terminal.shell", false),
            SettingEdit::remove("terminal.shell")
        );
        let out = apply_edits(source, &[reset_edit("terminal.shell", false)]).unwrap();
        assert_eq!(
            out,
            "[terminal]\n# Shell program.\n# Arguments.\nargs = []\n"
        );
        let (config, warnings) = Config::from_toml_str(&out);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(is_default(&config, "terminal.shell"));
        // The last key of its table: the comment stays where it was.
        let source = "[workspace]\nconfirm_delete = true\n# Where I keep my code.\nprojects_folder = \"~/code\"\n\n[search]\nmax_results = 10\n";
        let out = apply_edits(source, &[reset_edit("workspace.projects_folder", false)]).unwrap();
        assert_eq!(
            out,
            "[workspace]\nconfirm_delete = true\n# Where I keep my code.\n\n[search]\nmax_results = 10\n"
        );
    }

    #[test]
    fn low_end_mode_has_its_own_defaults() {
        let (config, _) = Config::from_toml_str("[ui]\nlow_end_mode = true\n");
        // Off because of low-end mode: that is the default then.
        assert!(!config.editor.inlay_hints);
        assert!(is_default(&config, "editor.inlay_hints"));
        assert!(is_default(&config, "editor.minimap"));
        // Low-end mode itself is not a default.
        assert!(!is_default(&config, "ui.low_end_mode"));
        // Turned on by hand in low-end mode: the reset turns it off with
        // the tag, so leaving low-end mode turns it back on.
        let source = "[ui]\nlow_end_mode = true\n[editor]\ninlay_hints = true\n";
        let (config, _) = Config::from_toml_str(source);
        assert!(!is_default(&config, "editor.inlay_hints"));
        let reset = apply_edits(source, &[reset_edit("editor.inlay_hints", true)]).unwrap();
        assert!(
            reset.contains("inlay_hints = false # [low-end mode]"),
            "{reset}"
        );
        let off = apply_edits(&reset, &crate::low_end_edits(false)).unwrap();
        assert!(Config::from_toml_str(&off).0.editor.inlay_hints, "{off}");
    }

    #[test]
    fn lists_are_written_and_compared() {
        let edit = SettingEdit::set(
            "todo.tags",
            SettingValue::List(vec!["TODO".into(), "FIXME".into()]),
        );
        let out = apply_edits(
            "[todo]\ntags = [\"X\"] # mine\n",
            std::slice::from_ref(&edit),
        )
        .unwrap();
        assert_eq!(out, "[todo]\ntags = [\"TODO\", \"FIXME\"] # mine\n");
        let (config, _) = Config::from_toml_str(&out);
        assert_eq!(config.todo.tags, ["TODO", "FIXME"]);
        // A guard compares lists item by item.
        let guarded = SettingEdit::set("todo.tags", SettingValue::List(Vec::new())).when(
            Guard::Is(SettingValue::List(vec!["TODO".into(), "FIXME".into()])),
        );
        let cleared = apply_edits(&out, &[guarded]).unwrap();
        assert!(cleared.contains("tags = [] # mine"), "{cleared}");
    }

    #[test]
    fn defaults_are_shown_in_words() {
        assert_eq!(shown_default("editor.tab_size", false), "4");
        assert_eq!(shown_default("editor.line_height", false), "1.5");
        assert_eq!(shown_default("ui.text_gamma", false), "1.8");
        assert_eq!(shown_default("editor.minimap", false), "on");
        assert_eq!(shown_default("editor.minimap", true), "off");
        assert_eq!(shown_default("editor.large_file_bytes", false), "4 MB");
        assert_eq!(shown_default("search.exclude", false), "empty");
        assert_eq!(shown_default("terminal.shell", false), "unset");
        assert_eq!(shown_default("ui.dark_theme", false), "Arcode Ember");
        assert_eq!(
            shown_default("todo.tags", false),
            "TODO, FIXME, HACK, XXX, BUG, NOTE"
        );
    }

    #[test]
    fn documented_keys_cover_every_fixed_section() {
        let keys = documented_keys();
        for key in [
            "editor.tab_size",
            "editor.completion.delay_ms",
            "search.max_results",
            "todo.tags",
            "plugins.network",
        ] {
            assert!(keys.iter().any(|k| k == key), "{key} missing");
        }
        assert!(!keys.iter().any(|k| k.starts_with("language")));
        assert!(!keys.iter().any(|k| k == "editor.completion"));
    }
}
