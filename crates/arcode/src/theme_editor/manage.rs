//! Making, renaming and deleting themes: the palette commands and actions,
//! the settings that follow a rename or a delete, and what the open theme
//! editor does when the theme it edits is renamed or deleted.

use arcode_config::theme::{DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME};
use arcode_config::{Appearance, SettingEdit, Theme, ThemeChoice, UiSettings};
use gpui::{Action, Context, SharedString, actions};

use super::ThemeEditor;

actions!(
    theme_editor,
    [
        /// Opens the theme editor on the theme in use.
        EditCurrentTheme,
        /// Asks for a name and a theme to start from, then makes a new
        /// theme and opens the theme editor on it.
        NewTheme,
    ]
);

/// Asks for a new name for the user theme `name`.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = theme_editor, no_json)]
pub struct RenameTheme {
    /// The theme's current name.
    pub name: SharedString,
}

/// Asks, then deletes the user theme `name` (its file).
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = theme_editor, no_json)]
pub struct DeleteTheme {
    /// The theme's name.
    pub name: SharedString,
}

/// Palette entries.
pub fn commands() -> Vec<(&'static str, Box<dyn Action>)> {
    vec![
        ("Theme: New Theme…", Box::new(NewTheme)),
        ("Theme: Edit Current Theme…", Box::new(EditCurrentTheme)),
    ]
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The settings that named `old` name `new` after a rename (the theme
/// picked by name, the dark theme, the light theme).
pub fn rename_edits(ui: &UiSettings, old: &str, new: &str) -> Vec<SettingEdit> {
    let mut edits = Vec::new();
    if matches!(&ui.theme, ThemeChoice::Named(name) if same(name, old)) {
        edits.push(SettingEdit::set("ui.theme", new));
    }
    if same(&ui.dark_theme, old) {
        edits.push(SettingEdit::set("ui.dark_theme", new));
    }
    if same(&ui.light_theme, old) {
        edits.push(SettingEdit::set("ui.light_theme", new));
    }
    edits
}

/// The settings after deleting `name` (an `appearance` theme): a theme
/// picked by name gives way to the dark or light mode it was shown in, and
/// a dark or light theme goes back to the default for its mode (Arcode
/// Ember, Arcode Chalk).
pub fn delete_edits(ui: &UiSettings, name: &str, appearance: Appearance) -> Vec<SettingEdit> {
    let mut edits = Vec::new();
    if matches!(&ui.theme, ThemeChoice::Named(n) if same(n, name)) {
        let mode = match appearance {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        };
        edits.push(SettingEdit::set("ui.theme", mode));
    }
    if same(&ui.dark_theme, name) {
        edits.push(SettingEdit::set("ui.dark_theme", DEFAULT_DARK_THEME));
    }
    if same(&ui.light_theme, name) {
        edits.push(SettingEdit::set("ui.light_theme", DEFAULT_LIGHT_THEME));
    }
    edits
}

impl ThemeEditor {
    /// The name of the theme being edited.
    pub fn theme_name(&self) -> &str {
        &self.edit.start().name
    }

    /// The user theme `old` is now called `new`: edits of it keep going,
    /// and saving writes its renamed file.
    pub fn theme_renamed(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        if self.edit.start().name == old {
            self.edit.rename(new);
            self.apply_now(cx);
        }
    }

    /// The user theme `name` was deleted: the page edits `fallback` (the
    /// theme in use now) instead, dropping any edits (the user agreed to
    /// delete the theme).
    pub fn theme_deleted(&mut self, name: &str, fallback: Theme, cx: &mut Context<Self>) {
        if self.edit.start().name == name {
            self.close_picker(cx);
            self.replace_edit(fallback, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui(toml: &str) -> UiSettings {
        let (config, warnings) = arcode_config::Config::from_toml_str(toml);
        assert!(warnings.is_empty(), "{warnings:?}");
        config.ui
    }

    #[test]
    fn a_rename_follows_every_setting_that_names_the_theme() {
        let settings = ui("[ui]\ntheme = \"Night\"\ndark_theme = \"night\"\n");
        assert_eq!(
            rename_edits(&settings, "Night", "Late Night"),
            vec![
                SettingEdit::set("ui.theme", "Late Night"),
                SettingEdit::set("ui.dark_theme", "Late Night"),
            ]
        );
        let settings = ui("[ui]\ntheme = \"system\"\nlight_theme = \"Paper\"\n");
        assert_eq!(
            rename_edits(&settings, "Paper", "Card"),
            vec![SettingEdit::set("ui.light_theme", "Card")]
        );
        assert!(rename_edits(&settings, "Other", "X").is_empty());
    }

    #[test]
    fn deleting_the_theme_in_use_falls_back_to_the_mode_default() {
        let settings = ui("[ui]\ntheme = \"Night\"\n");
        assert_eq!(
            delete_edits(&settings, "Night", Appearance::Dark),
            vec![SettingEdit::set("ui.theme", "dark")]
        );
        let settings = ui("[ui]\ntheme = \"Paper\"\n");
        assert_eq!(
            delete_edits(&settings, "Paper", Appearance::Light),
            vec![SettingEdit::set("ui.theme", "light")]
        );
        let settings =
            ui("[ui]\ntheme = \"system\"\ndark_theme = \"Night\"\nlight_theme = \"Paper\"\n");
        assert_eq!(
            delete_edits(&settings, "Night", Appearance::Dark),
            vec![SettingEdit::set("ui.dark_theme", "Arcode Ember")]
        );
        assert_eq!(
            delete_edits(&settings, "Paper", Appearance::Light),
            vec![SettingEdit::set("ui.light_theme", "Arcode Chalk")]
        );
        assert!(delete_edits(&settings, "Unused", Appearance::Dark).is_empty());
    }

    #[test]
    fn the_palette_lists_new_theme() {
        let names: Vec<&str> = commands().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"Theme: New Theme…"), "{names:?}");
    }
}
