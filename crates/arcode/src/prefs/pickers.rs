//! Items and results of the theme and font pickers (no UI here).

use arcode_config::theme::BUILTIN_THEME_NAMES;
use arcode_config::{Config, SettingEdit, Theme, ThemeChoice, ThemeRegistry};
use arcode_ui_kit::{BUNDLED_MONO_FONT, BUNDLED_UI_FONT, SYSTEM_UI_FONT, looks_monospace};

use crate::list_picker::PickItem;

/// Which theme setting a theme picker sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSlot {
    /// `ui.theme`: system, dark, light or a named theme.
    Main,
    /// `ui.dark_theme`.
    Dark,
    /// `ui.light_theme`.
    Light,
}

impl ThemeSlot {
    /// The config key written.
    pub fn key(self) -> &'static str {
        match self {
            Self::Main => "ui.theme",
            Self::Dark => "ui.dark_theme",
            Self::Light => "ui.light_theme",
        }
    }
}

/// One row of a theme picker.
pub type ThemeOption = ThemeChoice;

/// The rows of a theme picker and what each one chooses.
pub fn theme_items(
    slot: ThemeSlot,
    registry: &ThemeRegistry,
    config: &Config,
) -> (Vec<PickItem>, Vec<ThemeOption>) {
    let ui = &config.ui;
    let mut items = Vec::new();
    let mut options = Vec::new();
    if slot == ThemeSlot::Main {
        let fixed = [
            (
                ThemeChoice::System,
                "Follow System",
                format!("{} / {}", ui.dark_theme, ui.light_theme),
            ),
            (ThemeChoice::Dark, "Dark", ui.dark_theme.clone()),
            (ThemeChoice::Light, "Light", ui.light_theme.clone()),
        ];
        for (choice, label, detail) in fixed {
            items.push(PickItem::new(label, detail).current(ui.theme == choice));
            options.push(choice);
        }
    }
    let current = match slot {
        ThemeSlot::Main => match &ui.theme {
            ThemeChoice::Named(name) => Some(name.as_str()),
            _ => None,
        },
        ThemeSlot::Dark => Some(ui.dark_theme.as_str()),
        ThemeSlot::Light => Some(ui.light_theme.as_str()),
    };
    for theme in registry.themes() {
        let dark = theme.appearance.is_dark();
        let fits = match slot {
            ThemeSlot::Main => true,
            ThemeSlot::Dark => dark,
            ThemeSlot::Light => !dark,
        };
        if !fits {
            continue;
        }
        let mut detail = if dark { "dark" } else { "light" }.to_owned();
        if !BUILTIN_THEME_NAMES.contains(&theme.name.as_str()) {
            detail.push_str(" · themes folder");
        }
        let is_current = current.is_some_and(|c| c.eq_ignore_ascii_case(&theme.name));
        items.push(PickItem::new(theme.name.clone(), detail).current(is_current));
        options.push(ThemeChoice::Named(theme.name.clone()));
    }
    (items, options)
}

/// The theme shown while `option` is highlighted.
pub fn preview_theme<'a>(
    option: &ThemeOption,
    registry: &'a ThemeRegistry,
    config: &Config,
    system_dark: bool,
) -> &'a Theme {
    let ui = &config.ui;
    registry.resolve(option, &ui.dark_theme, &ui.light_theme, system_dark)
}

/// The edit that stores `option` in `slot`.
pub fn theme_edit(slot: ThemeSlot, option: &ThemeOption) -> SettingEdit {
    SettingEdit::set(slot.key(), option.as_str())
}

/// Which font family a font picker sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontTarget {
    /// `editor.font_family`.
    Editor,
    /// `ui.font_family` (unset: the bundled Inter).
    Ui,
    /// `terminal.font_family` (unset: the editor font).
    Terminal,
}

impl FontTarget {
    /// The config key written.
    pub fn key(self) -> &'static str {
        match self {
            Self::Editor => "editor.font_family",
            Self::Ui => "ui.font_family",
            Self::Terminal => "terminal.font_family",
        }
    }

    /// Placeholder of the picker.
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Editor => "Select Editor Font",
            Self::Ui => "Select UI Font",
            Self::Terminal => "Select Terminal Font",
        }
    }

    /// The family set now (`None` for the default of UI and terminal).
    pub fn current(self, config: &Config) -> Option<&str> {
        match self {
            Self::Editor => Some(config.editor.font_family.as_str()),
            Self::Ui => config.ui.font_family.as_deref(),
            Self::Terminal => config.terminal.font_family.as_deref(),
        }
    }
}

/// The rows of a font picker over `families` (see
/// `arcode_ui_kit::font_choices`). The returned values are the families,
/// with `""` for "the default" (UI and terminal fonts only).
pub fn font_items(
    target: FontTarget,
    families: &[String],
    config: &Config,
) -> (Vec<PickItem>, Vec<String>) {
    let current = target.current(config);
    let mut items = Vec::new();
    let mut values = Vec::new();
    let default = match target {
        FontTarget::Editor => None,
        FontTarget::Ui => Some((BUNDLED_UI_FONT, "bundled · default")),
        FontTarget::Terminal => Some(("Same as Editor", config.editor.font_family.as_str())),
    };
    if let Some((label, detail)) = default {
        // An explicit "Inter" renders the same as the unset default.
        let is_default =
            current.is_none() || (target == FontTarget::Ui && current == Some(BUNDLED_UI_FONT));
        items.push(PickItem::new(label, detail.to_owned()).current(is_default));
        values.push(String::new());
    }
    if target == FontTarget::Ui {
        // The platform UI font is a hidden family name, so offer it here.
        items.push(
            PickItem::new(SYSTEM_UI_FONT_LABEL, "platform")
                .current(current == Some(SYSTEM_UI_FONT)),
        );
        values.push(SYSTEM_UI_FONT.to_owned());
    }
    for family in families {
        if target == FontTarget::Ui && family == BUNDLED_UI_FONT {
            continue; // already the default row
        }
        let detail = if family == BUNDLED_MONO_FONT {
            "bundled · monospace"
        } else if family == BUNDLED_UI_FONT {
            "bundled"
        } else if looks_monospace(family) {
            "monospace"
        } else {
            ""
        };
        items.push(PickItem::new(family.clone(), detail).current(current == Some(family)));
        values.push(family.clone());
    }
    (items, values)
}

/// The picker row and Settings label of [`SYSTEM_UI_FONT`].
pub const SYSTEM_UI_FONT_LABEL: &str = "System UI Font";

/// The UI font as Settings shows it: the configured family, the bundled
/// Inter when unset, and "System UI Font" for the platform font.
pub fn ui_font_label(config: &Config) -> String {
    match config.ui.font_family.as_deref() {
        None => BUNDLED_UI_FONT.to_owned(),
        Some(SYSTEM_UI_FONT) => SYSTEM_UI_FONT_LABEL.to_owned(),
        Some(family) => family.to_owned(),
    }
}

/// `config` with `family` (`""` = default) in the target's setting, for a
/// live preview.
pub fn with_font(config: &Config, target: FontTarget, family: &str) -> Config {
    let mut config = config.clone();
    let value = (!family.is_empty()).then(|| family.to_owned());
    match target {
        FontTarget::Editor => {
            if let Some(family) = value {
                config.editor.font_family = family;
            }
        }
        FontTarget::Ui => config.ui.font_family = value,
        FontTarget::Terminal => config.terminal.font_family = value,
    }
    config
}

/// The edit that stores `family` (`""` removes the key: the default).
pub fn font_edit(target: FontTarget, family: &str) -> SettingEdit {
    if family.is_empty() {
        SettingEdit::remove(target.key())
    } else {
        SettingEdit::set(target.key(), family)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_theme_rows_start_with_the_modes() {
        let registry = ThemeRegistry::builtin();
        let config = Config::default();
        let (items, options) = theme_items(ThemeSlot::Main, &registry, &config);
        assert_eq!(items.len(), 3 + BUILTIN_THEME_NAMES.len());
        assert_eq!(options[0], ThemeChoice::System);
        assert!(items[0].current, "system is the default");
        assert_eq!(items[3].label.as_ref(), "Arcode Ember");
        assert_eq!(items[4].label.as_ref(), "Arcode Chalk");
        assert_eq!(
            theme_edit(ThemeSlot::Main, &options[7]),
            SettingEdit::set("ui.theme", "Arcode Midnight")
        );
        let midnight = preview_theme(&options[7], &registry, &config, false);
        assert_eq!(midnight.name, "Arcode Midnight");
        let light = preview_theme(&options[0], &registry, &config, false);
        assert_eq!(light.name, "Arcode Chalk");
    }

    #[test]
    fn slot_pickers_list_matching_appearances() {
        let registry = ThemeRegistry::builtin();
        let config = Config::default();
        let (items, options) = theme_items(ThemeSlot::Light, &registry, &config);
        assert_eq!(items.len(), 2);
        assert!(items[0].current);
        assert_eq!(
            theme_edit(ThemeSlot::Light, &options[0]),
            SettingEdit::set("ui.light_theme", "Arcode Chalk")
        );
        assert_eq!(
            theme_edit(ThemeSlot::Light, &options[1]),
            SettingEdit::set("ui.light_theme", "Arcode Light")
        );
        let (items, _) = theme_items(ThemeSlot::Dark, &registry, &config);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn font_rows_offer_the_default_and_mark_the_current() {
        let families =
            arcode_ui_kit::font_choices(["DejaVu Sans Mono", "Noto Sans"].map(String::from));
        let config = Config::default();
        let (items, values) = font_items(FontTarget::Editor, &families, &config);
        assert_eq!(values, ["JetBrains Mono", "DejaVu Sans Mono", "Noto Sans"]);
        assert!(items[0].current);
        assert_eq!(items[1].detail.as_ref(), "monospace");
        let (items, values) = font_items(FontTarget::Terminal, &families, &config);
        assert_eq!(values[0], "");
        assert!(items[0].current);
        assert_eq!(
            font_edit(FontTarget::Terminal, ""),
            SettingEdit::remove("terminal.font_family")
        );
        assert_eq!(
            font_edit(FontTarget::Ui, "Noto Sans"),
            SettingEdit::set("ui.font_family", "Noto Sans")
        );
        let preview = with_font(&config, FontTarget::Ui, "Noto Sans");
        assert_eq!(ui_font_label(&preview), "Noto Sans");
        assert_eq!(preview.ui.font_family.as_deref(), Some("Noto Sans"));
        assert_eq!(
            with_font(&config, FontTarget::Editor, "")
                .editor
                .font_family,
            "JetBrains Mono"
        );
    }

    #[test]
    fn ui_font_rows_name_inter_and_offer_the_platform_font() {
        let families = arcode_ui_kit::font_choices(["Inter", "Noto Sans"].map(String::from));
        let mut config = Config::default();
        assert_eq!(ui_font_label(&config), "Inter");
        let (items, values) = font_items(FontTarget::Ui, &families, &config);
        assert_eq!(values, ["", SYSTEM_UI_FONT, "JetBrains Mono", "Noto Sans"]);
        assert_eq!(items[0].label.as_ref(), "Inter");
        assert!(items[0].current && !items[1].current);
        assert_eq!(items[1].label.as_ref(), SYSTEM_UI_FONT_LABEL);
        assert_eq!(
            font_edit(FontTarget::Ui, &values[1]),
            SettingEdit::set("ui.font_family", SYSTEM_UI_FONT)
        );

        config.ui.font_family = Some(SYSTEM_UI_FONT.into());
        assert_eq!(ui_font_label(&config), SYSTEM_UI_FONT_LABEL);
        let (items, _) = font_items(FontTarget::Ui, &families, &config);
        assert!(!items[0].current && items[1].current);

        config.ui.font_family = Some("Inter".into());
        let (items, _) = font_items(FontTarget::Ui, &families, &config);
        assert!(items[0].current, "an explicit Inter is the default row");
        let (items, values) = font_items(FontTarget::Editor, &families, &config);
        assert_eq!(values[1], "Inter");
        assert_eq!(items[1].detail.as_ref(), "bundled");
    }
}
