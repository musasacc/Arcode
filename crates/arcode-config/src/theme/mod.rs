//! Themes: UI colors, syntax styles and terminal colors, loaded from TOML.
//!
//! A theme file looks like:
//!
//! ```toml
//! name = "My Theme"
//! appearance = "dark"          # or "light"
//! extends = "Arcode Midnight"  # optional; default: built-in of same appearance
//!
//! [ui]
//! background = "#101010"       # any key omitted is inherited
//!
//! [syntax]
//! keyword = { color = "#ff79c6", bold = true }
//! comment = "#6a737d"          # shorthand for { color = ... }
//!
//! [terminal]
//! foreground = "#e0e0e0"
//! bright_red = "#ff8080"
//! ```

mod color;
mod palette;
mod parse;
mod registry;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use toml::Table;

pub use color::{Color, ColorParseError};
pub use palette::{ANSI_NAMES, TerminalColors, UiColors};
pub use registry::{BUILTIN_THEME_NAMES, ThemeRegistry};

/// Whether a theme is dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Light text on a dark background.
    #[default]
    Dark,
    /// Dark text on a light background.
    Light,
}

impl Appearance {
    /// `true` for [`Appearance::Dark`].
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }

    /// Parses `"dark"` / `"light"` (case-insensitive).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

/// How a syntax capture is drawn. `color: None` uses the default text color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyntaxStyle {
    /// Foreground color.
    pub color: Option<Color>,
    /// Bold weight.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
}

/// A complete theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Display name, unique within a [`ThemeRegistry`].
    pub name: String,
    /// Dark or light.
    pub appearance: Appearance,
    /// Workbench colors.
    pub ui: UiColors,
    /// Styles keyed by tree-sitter capture name (e.g. `function.method`).
    pub syntax: BTreeMap<String, SyntaxStyle>,
    /// Integrated terminal colors.
    pub terminal: TerminalColors,
}

/// A theme that could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError(pub String);

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ThemeError {}

impl Theme {
    /// Parses a theme from TOML.
    ///
    /// With a `base`, any omitted color, syntax style or the appearance is
    /// inherited from it. Without one, every UI and terminal color is
    /// required. Unknown keys and invalid colors produce warnings.
    pub fn from_toml_str(
        source: &str,
        base: Option<&Theme>,
    ) -> Result<(Theme, Vec<String>), ThemeError> {
        let table = parse::parse_table(source)?;
        Self::from_table(&table, base, None)
    }

    /// Like [`Theme::from_toml_str`] for an already-parsed table;
    /// `fallback_name` is used when the table has no `name`.
    pub fn from_table(
        table: &Table,
        base: Option<&Theme>,
        fallback_name: Option<&str>,
    ) -> Result<(Theme, Vec<String>), ThemeError> {
        parse::theme_from_table(table, base, fallback_name)
    }

    /// Style for a capture name, falling back through its dotted prefixes:
    /// `function.method.builtin` → `function.method` → `function`.
    pub fn syntax_style(&self, capture_name: &str) -> Option<&SyntaxStyle> {
        let mut name = capture_name.strip_prefix('@').unwrap_or(capture_name);
        loop {
            if let Some(style) = self.syntax.get(name) {
                return Some(style);
            }
            name = name.rsplit_once('.')?.0;
        }
    }

    /// Color for a capture name (see [`Theme::syntax_style`]).
    pub fn syntax_color(&self, capture_name: &str) -> Option<Color> {
        self.syntax_style(capture_name).and_then(|s| s.color)
    }
}

#[cfg(test)]
mod tests;
