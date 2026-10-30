//! Built-in and user themes, and resolution of the configured choice.

use std::path::Path;
use std::sync::OnceLock;

use super::{Appearance, Theme, ThemeError, loader};
use crate::config::ThemeChoice;

/// Names of the built-in themes, in display order.
pub const BUILTIN_THEME_NAMES: [&str; 3] = ["Arcode Dark", "Arcode Light", "Arcode Midnight"];

const BUILTIN_SOURCES: [&str; 3] = [
    include_str!("../../themes/arcode-dark.toml"),
    include_str!("../../themes/arcode-light.toml"),
    include_str!("../../themes/arcode-midnight.toml"),
];

/// The parsed built-in themes (strict: every color present).
fn builtin_themes() -> &'static [Theme] {
    static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
    THEMES.get_or_init(|| {
        BUILTIN_SOURCES
            .iter()
            .map(|source| {
                // Built-in themes are compile-time data validated by tests.
                Theme::from_toml_str(source, None)
                    .map(|(theme, _)| theme)
                    .unwrap_or_else(|err| panic!("built-in theme is invalid: {err}"))
            })
            .collect()
    })
}

/// The built-in theme a user theme inherits from by default.
pub(super) fn builtin_base(appearance: Appearance) -> &'static Theme {
    let name = match appearance {
        Appearance::Dark => BUILTIN_THEME_NAMES[0],
        Appearance::Light => BUILTIN_THEME_NAMES[1],
    };
    let themes = builtin_themes();
    themes.iter().find(|t| t.name == name).unwrap_or(&themes[0])
}

/// All available themes: the built-ins followed by user themes.
#[derive(Debug, Clone)]
pub struct ThemeRegistry {
    themes: Vec<Theme>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ThemeRegistry {
    /// A registry with only the built-in themes.
    pub fn builtin() -> Self {
        Self {
            themes: builtin_themes().to_vec(),
        }
    }

    /// Built-ins plus every `*.toml` theme in `user_themes_dir` (if given).
    /// A missing directory is not an error; broken theme files are skipped
    /// with a warning. A user theme may replace a built-in by using its name.
    ///
    /// Files are read in path order (a later file defining the same name
    /// wins). `extends` may name any built-in or user theme regardless of
    /// file order; cycles produce a warning and fall back to the built-in.
    pub fn load(user_themes_dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut registry = Self::builtin();
        let warnings = match user_themes_dir {
            Some(dir) => loader::load_dir(&mut registry, dir),
            None => Vec::new(),
        };
        (registry, warnings)
    }

    /// Parses a user theme and adds it (replacing a theme of the same name).
    /// It inherits from `extends` (a theme already in the registry) if
    /// given, else from the built-in theme of its appearance. Returns the
    /// theme name and any warnings.
    pub fn add_user_theme(
        &mut self,
        source: &str,
        fallback_name: Option<&str>,
    ) -> Result<(String, Vec<String>), ThemeError> {
        let (theme, warnings) = loader::user_theme(self, source, fallback_name)?;
        let name = theme.name.clone();
        self.insert(theme);
        Ok((name, warnings))
    }

    /// Adds a theme, replacing (and returning) one with the same name.
    pub fn insert(&mut self, theme: Theme) -> Option<Theme> {
        match self.themes.iter_mut().find(|t| t.name == theme.name) {
            Some(slot) => Some(std::mem::replace(slot, theme)),
            None => {
                self.themes.push(theme);
                None
            }
        }
    }

    /// Theme names: built-ins first, then user themes in load order.
    pub fn names(&self) -> Vec<&str> {
        self.themes.iter().map(|t| t.name.as_str()).collect()
    }

    /// All themes in the order of [`ThemeRegistry::names`].
    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    /// A theme by name (exact match first, then case-insensitive).
    pub fn get(&self, name: &str) -> Option<&Theme> {
        let name = name.trim();
        self.themes.iter().find(|t| t.name == name).or_else(|| {
            self.themes
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        })
    }

    /// The default theme for an appearance (`Arcode Dark` / `Arcode Light`,
    /// or a user replacement of it).
    pub fn fallback(&self, appearance: Appearance) -> &Theme {
        let name = match appearance {
            Appearance::Dark => BUILTIN_THEME_NAMES[0],
            Appearance::Light => BUILTIN_THEME_NAMES[1],
        };
        self.get(name)
            .or_else(|| self.themes.iter().find(|t| t.appearance == appearance))
            .unwrap_or_else(|| builtin_base(appearance))
    }

    /// The theme to display for a [`ThemeChoice`]. Unknown names fall back to
    /// the default theme of the relevant appearance; never fails.
    pub fn resolve(
        &self,
        choice: &ThemeChoice,
        dark_theme: &str,
        light_theme: &str,
        system_is_dark: bool,
    ) -> &Theme {
        let system = if system_is_dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        let (name, appearance) = match choice {
            ThemeChoice::System if system_is_dark => (dark_theme, Appearance::Dark),
            ThemeChoice::System => (light_theme, Appearance::Light),
            ThemeChoice::Dark => (dark_theme, Appearance::Dark),
            ThemeChoice::Light => (light_theme, Appearance::Light),
            ThemeChoice::Named(name) => (name.as_str(), system),
        };
        self.get(name).unwrap_or_else(|| self.fallback(appearance))
    }
}
