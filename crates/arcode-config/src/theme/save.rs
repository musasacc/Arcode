//! Saving an edited theme into the user themes folder. Built-in themes are
//! compiled in and never written; a user theme is saved in its own file or,
//! for a new name, in `<themes dir>/<slug>.toml`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{BUILTIN_THEME_NAMES, Theme, parse};
use crate::fsutil::write_atomic;

/// The longest theme name the editor accepts.
pub const MAX_THEME_NAME: usize = 60;

/// Why a theme name or a save was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The name is empty or only spaces.
    EmptyName,
    /// The name is longer than [`MAX_THEME_NAME`] characters.
    NameTooLong,
    /// The name has control characters, or no letter or digit for a file
    /// name.
    InvalidName,
    /// The name is (case-insensitively) a built-in theme's.
    BuiltinName(String),
    /// Another file already uses the target path.
    Exists(PathBuf),
    /// The file could not be written.
    Io(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => f.write_str("Enter a name for the theme."),
            Self::NameTooLong => write!(f, "Use at most {MAX_THEME_NAME} characters."),
            Self::InvalidName => f.write_str("Use letters or digits in the name."),
            Self::BuiltinName(name) => {
                write!(f, "\"{name}\" is a built-in theme; choose another name.")
            }
            Self::Exists(path) => write!(f, "{} already exists.", path.display()),
            Self::Io(err) => write!(f, "Cannot save the theme: {err}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Whether `name` is a built-in theme's name (ignoring case and spaces at
/// either end).
pub fn is_builtin_name(name: &str) -> bool {
    let name = name.trim();
    BUILTIN_THEME_NAMES
        .iter()
        .any(|b| b.eq_ignore_ascii_case(name))
}

/// The file name stem for a theme name: lowercase ASCII letters and digits
/// with single dashes between words (`"My Ember 2"` → `my-ember-2`).
/// Other characters separate words; the result may be empty.
pub fn theme_slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Checks a name typed for "Save as": returns it trimmed.
pub fn validate_theme_name(name: &str) -> Result<String, SaveError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SaveError::EmptyName);
    }
    if name.chars().count() > MAX_THEME_NAME {
        return Err(SaveError::NameTooLong);
    }
    if name.chars().any(char::is_control) || theme_slug(name).is_empty() {
        return Err(SaveError::InvalidName);
    }
    if is_builtin_name(name) {
        return Err(SaveError::BuiltinName(name.to_owned()));
    }
    Ok(name.to_owned())
}

/// The `*.toml` file in `dir` that defines the theme `name` (its `name`
/// key, else its file stem), the last one in path order as the loader
/// keeps the last. `None` when no file does or `dir` cannot be read.
pub fn find_user_theme_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
                && p.is_file()
        })
        .collect();
    paths.sort();
    paths.into_iter().rev().find(|path| {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        fs::read_to_string(path)
            .ok()
            .and_then(|source| parse::parse_table(&source).ok())
            .and_then(|table| parse::theme_name(&table, stem.as_deref()))
            .is_some_and(|n| n == name)
    })
}

/// Where a theme is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveTarget {
    /// A new file `<dir>/<slug>.toml`; refused when the file exists unless
    /// `replace` (the user agreed to replace it).
    New { replace: bool },
    /// The user theme's own file (found with [`find_user_theme_file`]),
    /// or a new file when it has none.
    InPlace,
}

/// Writes `theme` (see [`Theme::to_toml_string`], leaving out `omit_ui`)
/// into the themes folder `dir` and returns the path. A built-in theme's
/// name is always refused, so a built-in theme is never written over.
pub fn save_user_theme(
    dir: &Path,
    theme: &Theme,
    omit_ui: &[&str],
    target: SaveTarget,
) -> Result<PathBuf, SaveError> {
    let name = validate_theme_name(&theme.name)?;
    let new_path = dir.join(format!("{}.toml", theme_slug(&name)));
    let path = match target {
        SaveTarget::InPlace => find_user_theme_file(dir, &name).unwrap_or(new_path),
        SaveTarget::New { replace } => {
            if !replace && new_path.exists() {
                return Err(SaveError::Exists(new_path));
            }
            new_path
        }
    };
    write_atomic(&path, theme.to_toml_string(omit_ui).as_bytes())
        .map_err(|err: io::Error| SaveError::Io(err.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThemeRegistry;

    fn ember() -> Theme {
        ThemeRegistry::builtin()
            .get("Arcode Ember")
            .unwrap()
            .clone()
    }

    #[test]
    fn slugs() {
        assert_eq!(theme_slug("My Ember"), "my-ember");
        assert_eq!(theme_slug("  Ünïcode -- Night!! 2 "), "n-code-night-2");
        assert_eq!(theme_slug("a/b\\c..d"), "a-b-c-d");
        assert_eq!(theme_slug("ÄÖÜ"), "");
        assert_eq!(theme_slug("---"), "");
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(validate_theme_name("  My Ember "), Ok("My Ember".into()));
        assert_eq!(validate_theme_name("   "), Err(SaveError::EmptyName));
        assert_eq!(validate_theme_name("ÄÖÜ"), Err(SaveError::InvalidName));
        assert_eq!(validate_theme_name("a\tb"), Err(SaveError::InvalidName));
        assert_eq!(
            validate_theme_name(&"x".repeat(MAX_THEME_NAME + 1)),
            Err(SaveError::NameTooLong)
        );
        for name in BUILTIN_THEME_NAMES {
            assert!(matches!(
                validate_theme_name(&name.to_uppercase()),
                Err(SaveError::BuiltinName(_))
            ));
        }
        assert!(SaveError::EmptyName.to_string().contains("name"));
    }

    #[test]
    fn save_as_writes_a_new_file_and_never_a_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let mut theme = ember();
        // A built-in name is refused whatever the target.
        for target in [SaveTarget::InPlace, SaveTarget::New { replace: true }] {
            let err = save_user_theme(dir.path(), &theme, &[], target).unwrap_err();
            assert!(matches!(err, SaveError::BuiltinName(_)));
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);

        theme.name = "My Ember".into();
        let path =
            save_user_theme(dir.path(), &theme, &[], SaveTarget::New { replace: false }).unwrap();
        assert_eq!(path, dir.path().join("my-ember.toml"));
        let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
        assert!(warnings.is_empty(), "{warnings:?}");
        let loaded = registry.get("My Ember").unwrap();
        assert_eq!(loaded.ui, theme.ui);
        assert_eq!(loaded.syntax, theme.syntax);
        // The built-in is untouched.
        assert_eq!(registry.get("Arcode Ember").unwrap(), &ember());

        // Saving as the same name again needs consent.
        let again = save_user_theme(dir.path(), &theme, &[], SaveTarget::New { replace: false });
        assert_eq!(again, Err(SaveError::Exists(path.clone())));
        assert!(
            save_user_theme(dir.path(), &theme, &[], SaveTarget::New { replace: true }).is_ok()
        );
    }

    #[test]
    fn in_place_finds_the_themes_own_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("handmade.toml"),
            "name = \"Night Shift\"\nappearance = \"dark\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("other.toml"), "name = \"Other\"\n").unwrap();
        fs::write(dir.path().join("broken.toml"), "name = [").unwrap();
        fs::write(
            dir.path().join("stem-only.toml"),
            "appearance = \"light\"\n",
        )
        .unwrap();
        assert_eq!(
            find_user_theme_file(dir.path(), "Night Shift"),
            Some(dir.path().join("handmade.toml"))
        );
        assert_eq!(
            find_user_theme_file(dir.path(), "stem-only"),
            Some(dir.path().join("stem-only.toml"))
        );
        assert_eq!(find_user_theme_file(dir.path(), "Missing"), None);
        assert_eq!(
            find_user_theme_file(&dir.path().join("nope"), "Other"),
            None
        );

        let mut theme = ember();
        theme.name = "Night Shift".into();
        theme.ui.accent = crate::Color::rgb(1, 2, 3);
        let path = save_user_theme(dir.path(), &theme, &[], SaveTarget::InPlace).unwrap();
        assert_eq!(path, dir.path().join("handmade.toml"));
        let (registry, _) = ThemeRegistry::load(Some(dir.path()));
        assert_eq!(
            registry.get("Night Shift").unwrap().ui.accent,
            theme.ui.accent
        );
        // A user theme without a file of its own gets a new one.
        theme.name = "Fresh".into();
        let path = save_user_theme(dir.path(), &theme, &[], SaveTarget::InPlace).unwrap();
        assert_eq!(path, dir.path().join("fresh.toml"));
    }
}
