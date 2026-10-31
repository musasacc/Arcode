//! Groups of settings written together: the low-end preset and the
//! choices of the first-run dialog.

use toml::Table;

use super::Config;
use super::edit_value::{Guard, SettingEdit, apply_edits};
use super::sections::{Keymap, ThemeChoice};

/// Settings low-end mode turns off. A key the file does not set is off
/// anyway while low-end mode is on (see [`apply_low_end_defaults`]).
const LOW_END_OFF: [&str; 2] = ["editor.inlay_hints", "git.inline_blame"];

/// The edits that turn low-end mode on or off. On also turns off inlay
/// hints and inline blame where the file turns them on, tagging each value
/// it changes with [`super::PRESET_MARK`]; off turns back on only the
/// values it tagged, so a setting the user turned off stays off. (The
/// cursor never blinks in low-end mode; the file is not changed for it.)
pub fn low_end_edits(on: bool) -> Vec<SettingEdit> {
    let mut edits = vec![SettingEdit::set("ui.low_end_mode", on)];
    for key in LOW_END_OFF {
        edits.push(if on {
            SettingEdit::set(key, false).when(Guard::IsThenMark(true.into()))
        } else {
            SettingEdit::set(key, true).when(Guard::Marked)
        });
    }
    edits
}

/// The edits that choose a keymap. Default also turns off `vim_mode`, which
/// would keep Vim on.
pub fn keymap_edits(keymap: Keymap) -> Vec<SettingEdit> {
    match keymap {
        Keymap::Default => vec![
            SettingEdit::set("editor.keymap", "default"),
            SettingEdit::set("editor.vim_mode", false).when(Guard::Is(true.into())),
        ],
        Keymap::Vim => vec![SettingEdit::set("editor.keymap", "vim")],
    }
}

/// Applies what low-end mode implies after a file is loaded: the cursor
/// never blinks, and inlay hints and inline blame are off unless the file
/// turns them on itself.
pub(super) fn apply_low_end_defaults(config: &mut Config, table: &Table) {
    if !config.ui.low_end_mode {
        return;
    }
    config.editor.cursor_blink = false;
    let set = |section: &str, key: &str| {
        table
            .get(section)
            .and_then(|s| s.as_table())
            .is_some_and(|s| s.contains_key(key))
    };
    if !set("editor", "inlay_hints") {
        config.editor.inlay_hints = false;
    }
    if !set("git", "inline_blame") {
        config.git.inline_blame = false;
    }
}

/// What the first-run dialog asks.
#[derive(Clone, Debug, PartialEq)]
pub struct FirstRunChoices {
    /// `ui.theme`.
    pub theme: ThemeChoice,
    /// `editor.font_family`.
    pub editor_font: String,
    /// `editor.keymap`.
    pub keymap: Keymap,
    /// `ui.low_end_mode` (with the rest of the low-end preset).
    pub low_end: bool,
}

impl Default for FirstRunChoices {
    fn default() -> Self {
        let config = Config::default();
        Self {
            theme: config.ui.theme,
            editor_font: config.editor.font_family,
            keymap: config.editor.keymap,
            low_end: config.ui.low_end_mode,
        }
    }
}

impl FirstRunChoices {
    /// The choices `config` makes (Vim when `vim_mode` is on).
    pub fn from_config(config: &Config) -> Self {
        let vim = config.editor.vim_mode || config.editor.keymap == Keymap::Vim;
        Self {
            theme: config.ui.theme.clone(),
            editor_font: config.editor.font_family.clone(),
            keymap: if vim { Keymap::Vim } else { Keymap::Default },
            low_end: config.ui.low_end_mode,
        }
    }

    /// The edits that record these choices.
    pub fn edits(&self) -> Vec<SettingEdit> {
        let mut edits = vec![
            SettingEdit::set("ui.theme", self.theme.as_str()),
            SettingEdit::set("editor.font_family", self.editor_font.as_str()),
        ];
        edits.extend(keymap_edits(self.keymap));
        edits.extend(low_end_edits(self.low_end));
        edits
    }

    /// The commented default template with these choices filled in.
    pub fn config_toml(&self) -> String {
        let template = Config::default_toml();
        // The template is valid TOML (tested), so this cannot fail.
        apply_edits(&template, &self.edits()).unwrap_or(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_choices_are_written_into_the_template() {
        let choices = FirstRunChoices {
            theme: ThemeChoice::Named("Arcode Midnight".into()),
            editor_font: "DejaVu Sans Mono".into(),
            keymap: Keymap::Vim,
            low_end: true,
        };
        let text = choices.config_toml();
        assert!(text.starts_with("# Arcode configuration"));
        let (config, warnings) = Config::from_toml_str(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.ui.theme, choices.theme);
        assert_eq!(config.editor.font_family, "DejaVu Sans Mono");
        assert_eq!(config.editor.keymap, Keymap::Vim);
        assert!(config.ui.low_end_mode);
        assert!(!config.editor.inlay_hints && !config.git.inline_blame);
        assert_eq!(config.ui.effective_animation_ms(), 0);
    }

    #[test]
    fn default_choices_give_the_default_config() {
        let (config, warnings) = Config::from_toml_str(&FirstRunChoices::default().config_toml());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn low_end_mode_changes_unset_defaults_only() {
        let (config, _) = Config::from_toml_str("[ui]\nlow_end_mode = true\n");
        assert!(!config.editor.inlay_hints);
        assert!(!config.git.inline_blame);
        let (config, _) = Config::from_toml_str(
            "[ui]\nlow_end_mode = true\n[editor]\ninlay_hints = true\ncursor_blink = true\n",
        );
        assert!(config.editor.inlay_hints, "set explicitly");
        assert!(!config.editor.cursor_blink, "never blinks in low-end mode");
        assert!(!config.git.inline_blame);
    }

    #[test]
    fn toggling_the_preset_round_trips() {
        let on = apply_edits(&Config::default_toml(), &low_end_edits(true)).unwrap();
        let (config, _) = Config::from_toml_str(&on);
        assert!(config.ui.low_end_mode && !config.editor.inlay_hints);
        let off = apply_edits(&on, &low_end_edits(false)).unwrap();
        let (config, _) = Config::from_toml_str(&off);
        assert!(!config.ui.low_end_mode && config.editor.inlay_hints && config.git.inline_blame);
        assert_eq!(
            off,
            Config::default_toml(),
            "the template comes back unchanged"
        );
    }

    #[test]
    fn low_end_keeps_what_the_user_turned_off() {
        let source = "[editor]\ninlay_hints = false\n[git]\ninline_blame = true # mine\n";
        let on = apply_edits(source, &low_end_edits(true)).unwrap();
        assert!(
            on.contains("inline_blame = false # mine [low-end mode]"),
            "{on}"
        );
        let off = apply_edits(&on, &low_end_edits(false)).unwrap();
        let (config, _) = Config::from_toml_str(&off);
        assert!(!config.editor.inlay_hints && config.git.inline_blame);
        assert!(off.contains("inline_blame = true # mine\n"), "{off}");
        // Unset keys are not written: low-end mode turns them off on load.
        let on = apply_edits("", &low_end_edits(true)).unwrap();
        assert_eq!(on.trim(), "[ui]\nlow_end_mode = true");
    }

    #[test]
    fn keymap_default_turns_vim_mode_off() {
        let source = "[editor]\nvim_mode = true\n";
        let out = apply_edits(source, &keymap_edits(Keymap::Default)).unwrap();
        let (config, _) = Config::from_toml_str(&out);
        assert!(!config.editor.vim_mode && config.editor.keymap == Keymap::Default);
        let out = apply_edits("", &keymap_edits(Keymap::Default)).unwrap();
        assert!(!out.contains("vim_mode"), "{out}");
    }

    #[test]
    fn choices_from_a_config() {
        let (config, _) = Config::from_toml_str(
            "[ui]\ntheme = \"Arcode Midnight\"\nlow_end_mode = true\n[editor]\nvim_mode = true\n",
        );
        let choices = FirstRunChoices::from_config(&config);
        assert_eq!(choices.theme, ThemeChoice::Named("Arcode Midnight".into()));
        assert!(choices.low_end && choices.keymap == Keymap::Vim);
        let normal = FirstRunChoices {
            low_end: false,
            ..choices
        };
        let out = apply_edits("[ui]\nlow_end_mode = true\n", &normal.edits()).unwrap();
        assert!(!Config::from_toml_str(&out).0.ui.low_end_mode);
    }
}
