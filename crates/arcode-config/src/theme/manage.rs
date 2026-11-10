//! Renaming and deleting the user's own themes (files in the themes
//! folder). Built-in themes are compiled in and are never renamed or
//! deleted.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, value};

use super::save::{SaveError, find_user_theme_file, is_builtin_name, theme_slug, user_theme_files};
use super::{Theme, ThemeRegistry, validate_theme_name};
use crate::fsutil::write_atomic;

/// Renames the user theme `old` to `new` (checked like a "Save as" name):
/// its file gets the new name in its `name` key, keeping everything else
/// as written, and moves to `<new slug>.toml`. A different file already at
/// that path, or another file that defines a theme of the new name
/// (ignoring case, as lookups do), is refused with [`SaveError::Exists`]
/// unless `replace`: then those files are deleted, so no other theme of
/// that name is left to win over the renamed one. Returns its new path.
pub fn rename_user_theme(
    dir: &Path,
    old: &str,
    new: &str,
    replace: bool,
) -> Result<PathBuf, SaveError> {
    if is_builtin_name(old) {
        return Err(SaveError::BuiltinName(old.to_owned()));
    }
    let new = validate_theme_name(new)?;
    let from = find_user_theme_file(dir, old)
        .ok_or_else(|| SaveError::Io(format!("{old} has no file in the themes folder")))?;
    let source = fs::read_to_string(&from).map_err(|e| SaveError::Io(e.to_string()))?;
    let mut doc: DocumentMut = source
        .parse()
        .map_err(|e: toml_edit::TomlError| SaveError::Io(e.to_string()))?;
    doc["name"] = value(new.as_str());
    let to = dir.join(format!("{}.toml", theme_slug(&new)));
    let same_file = to == from || same_path(&to, &from);
    if !same_file && !replace && to.exists() {
        return Err(SaveError::Exists(to));
    }
    // Other files of that name, whatever their file names.
    let taken: Vec<PathBuf> = user_theme_files(dir)
        .into_iter()
        .filter(|(path, name)| {
            name.eq_ignore_ascii_case(&new)
                && *path != from
                && *path != to
                && !same_path(path, &from)
                && !same_path(path, &to)
        })
        .map(|(path, _)| path)
        .collect();
    if let (Some(path), false) = (taken.first(), replace) {
        return Err(SaveError::Exists(path.clone()));
    }
    let text = renamed_header(&doc.to_string(), old, &new);
    write_atomic(&to, text.as_bytes()).map_err(|e| SaveError::Io(e.to_string()))?;
    for path in taken.iter().chain((!same_file).then_some(&from)) {
        fs::remove_file(path).map_err(|e| SaveError::Io(e.to_string()))?;
    }
    Ok(to)
}

/// The theme editor's first line (`# <name>: saved by the Arcode theme
/// editor.`) with the new name; any other text is kept as it is.
fn renamed_header(text: &str, old: &str, new: &str) -> String {
    let header = |name: &str| format!("# {name}: saved by the Arcode theme editor.");
    match text.strip_prefix(&header(old)) {
        Some(rest) => format!("{}{rest}", header(new)),
        None => text.to_owned(),
    }
}

/// Deletes the file of the user theme `name` and returns its path.
pub fn delete_user_theme(dir: &Path, name: &str) -> Result<PathBuf, SaveError> {
    if is_builtin_name(name) {
        return Err(SaveError::BuiltinName(name.to_owned()));
    }
    let path = find_user_theme_file(dir, name)
        .ok_or_else(|| SaveError::Io(format!("{name} has no file in the themes folder")))?;
    fs::remove_file(&path).map_err(|e| SaveError::Io(e.to_string()))?;
    Ok(path)
}

/// Whether two paths name the same file (a name differing only in case on
/// a file system that ignores case).
fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl ThemeRegistry {
    /// Removes the user theme `name` and returns it; built-in themes stay.
    pub fn remove_user_theme(&mut self, name: &str) -> Option<Theme> {
        if is_builtin_name(name) {
            return None;
        }
        let ix = self.themes.iter().position(|t| t.name == name)?;
        Some(self.themes.remove(ix))
    }

    /// Removes every user theme named `name`, ignoring case (the themes a
    /// rename replaced); built-in themes stay.
    pub fn remove_user_themes_named(&mut self, name: &str) {
        self.themes
            .retain(|t| is_builtin_name(&t.name) || !t.name.eq_ignore_ascii_case(name.trim()));
    }

    /// Whether `name` is one of the user's own themes (not built in).
    pub fn is_user_theme(&self, name: &str) -> bool {
        !is_builtin_name(name) && self.themes.iter().any(|t| t.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{SaveTarget, save_user_theme};

    fn ember() -> Theme {
        ThemeRegistry::builtin()
            .get("Arcode Ember")
            .unwrap()
            .clone()
    }

    fn saved(dir: &Path, name: &str) -> PathBuf {
        let theme = Theme {
            name: name.into(),
            ..ember()
        };
        save_user_theme(dir, &theme, &[], SaveTarget::New { replace: false }).unwrap()
    }

    #[test]
    fn renaming_moves_the_file_and_keeps_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let hand = dir.path().join("hand.toml");
        fs::write(
            &hand,
            "# my colours\nname = \"Night\"\nappearance = \"dark\"\n\n[ui]\naccent = \"#123456\"\n",
        )
        .unwrap();
        let to = rename_user_theme(dir.path(), "Night", "  Late Night ", false).unwrap();
        assert_eq!(to, dir.path().join("late-night.toml"));
        assert!(!hand.exists());
        let text = fs::read_to_string(&to).unwrap();
        assert!(
            text.starts_with("# my colours\nname = \"Late Night\"\n"),
            "{text}"
        );
        assert!(text.contains("accent = \"#123456\""), "{text}");
        let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(registry.get("Late Night").is_some());
        assert!(!registry.is_user_theme("Night"));

        // A file named by its stem gets a `name` key.
        fs::write(dir.path().join("stem.toml"), "appearance = \"light\"\n").unwrap();
        let to = rename_user_theme(dir.path(), "stem", "Stem Light", false).unwrap();
        let text = fs::read_to_string(to).unwrap();
        assert!(text.contains("name = \"Stem Light\""), "{text}");
    }

    #[test]
    fn renaming_in_place_and_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let path = saved(dir.path(), "My Ember");
        // Only the spelling changes: the same file.
        let to = rename_user_theme(dir.path(), "My Ember", "my ember", false).unwrap();
        assert_eq!(to, path);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"my ember\""), "{text}");
        assert!(
            text.starts_with("# my ember: saved by the Arcode theme editor.\n"),
            "{text}"
        );

        let other = saved(dir.path(), "Other");
        let err = rename_user_theme(dir.path(), "my ember", "Other", false).unwrap_err();
        assert_eq!(err, SaveError::Exists(other.clone()));
        rename_user_theme(dir.path(), "my ember", "Other", true).unwrap();
        assert!(!path.exists());
        let (registry, _) = ThemeRegistry::load(Some(dir.path()));
        assert_eq!(registry.get("Other").unwrap().name, "Other");

        assert!(matches!(
            rename_user_theme(dir.path(), "Arcode Ember", "Mine", false),
            Err(SaveError::BuiltinName(_))
        ));
        assert!(matches!(
            rename_user_theme(dir.path(), "Other", "arcode chalk", false),
            Err(SaveError::BuiltinName(_))
        ));
        assert_eq!(
            rename_user_theme(dir.path(), "Other", " ", false),
            Err(SaveError::EmptyName)
        );
        assert!(matches!(
            rename_user_theme(dir.path(), "Missing", "Found", false),
            Err(SaveError::Io(_))
        ));
    }

    // Replacing a theme whose file is not named after it: before, its file
    // stayed, and after a restart it could win over the renamed theme.
    #[test]
    fn replacing_a_theme_deletes_its_file_whatever_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let night = dir.path().join("night.toml");
        let write = |path: &Path, name: &str, accent: &str| {
            let text = format!(
                "name = \"{name}\"\nappearance = \"dark\"\n\n[ui]\naccent = \"{accent}\"\n"
            );
            fs::write(path, text).unwrap();
        };
        write(&night, "Late Night", "#111111");
        write(&dir.path().join("my-ember.toml"), "My Ember", "#ee0000");
        // A different spelling of the name is the same theme.
        let shout = dir.path().join("shout.toml");
        write(&shout, "LATE NIGHT", "#222222");
        let err = rename_user_theme(dir.path(), "My Ember", "Late Night", false).unwrap_err();
        assert!(
            matches!(&err, SaveError::Exists(p) if *p == night || *p == shout),
            "{err:?}"
        );
        assert!(night.exists() && shout.exists());

        let to = rename_user_theme(dir.path(), "My Ember", "Late Night", true).unwrap();
        let mut files: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["late-night.toml"]);
        let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
        assert!(warnings.is_empty(), "{warnings:?}");
        let theme = registry.get("Late Night").unwrap();
        assert_eq!(theme.ui.accent, crate::Color::rgb(0xee, 0, 0));
        assert_eq!(to, dir.path().join("late-night.toml"));
    }

    #[test]
    fn deleting_removes_only_the_users_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = saved(dir.path(), "Gone Soon");
        let kept = saved(dir.path(), "Kept");
        assert_eq!(delete_user_theme(dir.path(), "Gone Soon"), Ok(path.clone()));
        assert!(!path.exists());
        assert!(kept.exists());
        assert!(matches!(
            delete_user_theme(dir.path(), "Arcode Midnight"),
            Err(SaveError::BuiltinName(_))
        ));
        assert!(matches!(
            delete_user_theme(dir.path(), "Gone Soon"),
            Err(SaveError::Io(_))
        ));
    }

    #[test]
    fn the_registry_forgets_user_themes_only() {
        let mut registry = ThemeRegistry::builtin();
        registry.insert(Theme {
            name: "Mine".into(),
            ..ember()
        });
        assert!(registry.is_user_theme("Mine"));
        assert!(!registry.is_user_theme("Arcode Ember"));
        assert!(registry.remove_user_theme("Arcode Ember").is_none());
        assert_eq!(registry.remove_user_theme("Mine").unwrap().name, "Mine");
        assert!(registry.get("Mine").is_none());
        assert!(registry.remove_user_theme("Mine").is_none());

        for name in ["Other", "OTHER"] {
            registry.insert(Theme {
                name: name.into(),
                ..ember()
            });
        }
        registry.remove_user_themes_named("other");
        registry.remove_user_themes_named("arcode ember");
        assert!(registry.get("other").is_none());
        assert!(registry.get("Arcode Ember").is_some());
    }
}
