//! Writing a theme back as TOML, in the layout of the built-in theme files
//! (`name`, `appearance`, then complete `[ui]`, `[syntax]` and `[terminal]`
//! tables), so a saved theme reads like the ones Arcode ships.

use std::fmt::Write as _;

use toml::Value;

use super::registry::builtin_base;
use super::tokens::ordered_captures;
use super::{SyntaxStyle, TerminalColors, Theme, UiColors};

/// A TOML key: bare when it can be, else quoted (`"function.method"`).
fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if bare {
        name.to_owned()
    } else {
        Value::String(name.to_owned()).to_string()
    }
}

/// `{ color = "#e6a0c8", italic = true }`. A style without a colour keeps
/// one flag written, so it still parses as a style of its own.
fn style(style: &SyntaxStyle) -> String {
    let mut parts = Vec::new();
    if let Some(color) = style.color {
        parts.push(format!("color = \"{}\"", color.to_hex()));
    }
    for (name, on) in [
        ("bold", style.bold),
        ("italic", style.italic),
        ("underline", style.underline),
    ] {
        if on {
            parts.push(format!("{name} = true"));
        }
    }
    if parts.is_empty() {
        parts.push("bold = false".into());
    }
    format!("{{ {} }}", parts.join(", "))
}

impl Theme {
    /// The theme as a TOML theme file that parses back to the same theme
    /// (with or without a base theme). `[ui]` keys in `omit_ui` are left
    /// out, so a derivable key (see `derived_keys`) is derived from the
    /// written colours when the file is loaded.
    ///
    /// Captures the base theme of the same appearance styles and this theme
    /// does not are written with the style they are drawn with here (found
    /// through their dotted prefixes), so loading the file as a user theme,
    /// which inherits the base's captures, draws the same colours.
    pub fn to_toml_string(&self, omit_ui: &[&str]) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# {}: saved by the Arcode theme editor.", self.name);
        let _ = writeln!(out, "name = {}", Value::String(self.name.clone()));
        let appearance = if self.appearance.is_dark() {
            "dark"
        } else {
            "light"
        };
        let _ = writeln!(out, "appearance = \"{appearance}\"");

        out.push_str("\n[ui]\n");
        for field in UiColors::FIELDS {
            if omit_ui.contains(field) {
                continue;
            }
            if let Some(color) = self.ui.get(field) {
                let _ = writeln!(out, "{field} = \"{}\"", color.to_hex());
            }
        }

        out.push_str("\n[syntax]\n");
        let base = builtin_base(self.appearance);
        let mut names: Vec<&String> = self.syntax.keys().collect();
        names.extend(base.syntax.keys().filter(|k| !self.syntax.contains_key(*k)));
        for name in ordered_captures(names) {
            let shown = self
                .syntax
                .get(&name)
                .or_else(|| self.syntax_style(&name))
                .copied()
                .unwrap_or_default();
            let _ = writeln!(out, "{} = {}", key(&name), style(&shown));
        }

        out.push_str("\n[terminal]\n");
        // Background first, as in the built-in files.
        let mut fields = TerminalColors::FIELDS;
        fields.swap(0, 1);
        for field in fields {
            if let Some(color) = self.terminal.get(field) {
                let _ = writeln!(out, "{field} = \"{}\"", color.to_hex());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BUILTIN_THEME_NAMES, Color, ThemeRegistry, derive};
    use super::*;

    fn builtin(name: &str) -> Theme {
        ThemeRegistry::builtin().get(name).unwrap().clone()
    }

    #[test]
    fn every_builtin_round_trips() {
        for name in BUILTIN_THEME_NAMES {
            let theme = builtin(name);
            let text = theme.to_toml_string(&[]);
            // Strict: every colour is in the file.
            let (strict, warnings) = Theme::from_toml_str(&text, None).unwrap();
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
            assert_eq!(strict, theme, "{name}");
            // As a user theme on top of its base.
            let mut registry = ThemeRegistry::builtin();
            let renamed = text.replace(name, "Round Trip");
            let (loaded, warnings) = registry.add_user_theme(&renamed, None).unwrap();
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
            let user = registry.get(&loaded).unwrap();
            assert_eq!(user.ui, theme.ui, "{name}");
            assert_eq!(user.terminal, theme.terminal, "{name}");
            assert_eq!(user.syntax, theme.syntax, "{name}");
            assert_eq!(user.appearance, theme.appearance, "{name}");
            // Writing again gives the same text.
            assert_eq!(strict.to_toml_string(&[]), text, "{name}");
        }
    }

    #[test]
    fn the_layout_matches_the_builtin_files() {
        let text = builtin("Arcode Ember").to_toml_string(&[]);
        assert!(text.contains(
            "name = \"Arcode Ember\"\nappearance = \"dark\"\n\n[ui]\nbackground = \"#25201b\"\n"
        ));
        assert!(text.contains("\n[syntax]\nkeyword = { color = \"#e6a0c8\" }\n"));
        assert!(text.contains("\"function.method\" = { color = \"#6cc8a2\" }\n"));
        assert!(text.contains("comment = { color = \"#ab9f8e\", italic = true }\n"));
        assert!(
            text.contains("\n[terminal]\nbackground = \"#25201b\"\nforeground = \"#efe7da\"\n")
        );
        assert!(text.ends_with("bright_white = \"#fbf6ee\"\n"));
        // Translucent colours keep their alpha.
        let dark = builtin("Arcode Dark").to_toml_string(&[]);
        assert!(dark.contains("selection = \"#5b9cf54d\""));
    }

    #[test]
    fn omitted_keys_are_derived_on_load() {
        let mut theme = builtin("Arcode Ember");
        theme.ui.title_bar = Color::rgb(1, 2, 3);
        theme.ui.canvas = derive::derived_value(&theme.ui, "canvas").unwrap();
        let text = theme.to_toml_string(&["canvas"]);
        assert!(!text.contains("\ncanvas ="));
        let (loaded, _) = Theme::from_toml_str(&text, Some(&builtin("Arcode Chalk"))).unwrap();
        assert_eq!(loaded.ui.canvas, Color::rgb(1, 2, 3));
        assert_eq!(loaded, theme);
    }

    #[test]
    fn names_and_odd_styles_are_escaped() {
        let mut theme = builtin("Arcode Chalk");
        theme.name = "Quote \"me\" \\ here".into();
        theme
            .syntax
            .insert("odd key".into(), SyntaxStyle::default());
        theme.syntax.insert(
            "markup.bold".into(),
            SyntaxStyle {
                color: None,
                bold: true,
                italic: false,
                underline: true,
            },
        );
        let text = theme.to_toml_string(&[]);
        assert!(text.contains("\"odd key\" = { bold = false }"), "{text}");
        assert!(text.contains("\"markup.bold\" = { bold = true, underline = true }"));
        let (loaded, warnings) = Theme::from_toml_str(&text, None).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded, theme);
    }

    #[test]
    fn captures_missing_here_keep_their_drawn_style() {
        let mut theme = builtin("Arcode Ember");
        theme.syntax.remove("function.method").unwrap();
        theme.syntax.get_mut("function").unwrap().color = Some(Color::rgb(9, 9, 9));
        let function = theme.syntax["function"];
        let text = theme.to_toml_string(&[]);
        let mut registry = ThemeRegistry::builtin();
        let renamed = text.replace("Arcode Ember", "Partial");
        registry.add_user_theme(&renamed, None).unwrap();
        // Drawn like `function`, as before saving, not with the base's own.
        let user = registry.get("Partial").unwrap();
        assert_eq!(user.syntax_style("function.method"), Some(&function));
    }
}
