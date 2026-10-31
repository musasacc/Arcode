//! Small, comment-preserving edits of `config.toml` made from the UI (the
//! Languages page and the language picker). Everything else in the file,
//! comments and layout included, stays as the user wrote it.

use std::io;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, TableLike, value};

use crate::fsutil::write_atomic;

/// Sets `[language.<id>].enabled` for each id. `true` removes the key (on
/// is the default) instead of writing it.
pub fn set_languages_enabled(source: &str, ids: &[&str], enabled: bool) -> Result<String, String> {
    let mut doc = parse(source)?;
    let languages = table_entry(doc.as_table_mut(), "language")?;
    for id in ids {
        let id = id.to_lowercase();
        if enabled {
            if let Some(table) = languages.get_mut(&id).and_then(Item::as_table_like_mut) {
                table.remove("enabled");
            }
        } else {
            let table = table_entry(languages, &id)?;
            table.insert("enabled", value(false));
        }
    }
    Ok(doc.to_string())
}

/// Sets (`Some(language)`) or removes (`None`) the file association of
/// `pattern` in `[files].associations`.
pub fn set_file_association(
    source: &str,
    pattern: &str,
    language: Option<&str>,
) -> Result<String, String> {
    let mut doc = parse(source)?;
    let files = table_entry(doc.as_table_mut(), "files")?;
    match language {
        Some(language) => {
            if files.get("associations").is_none() {
                files.insert(
                    "associations",
                    Item::Value(toml_edit::InlineTable::new().into()),
                );
            }
            let associations = files
                .get_mut("associations")
                .and_then(Item::as_table_like_mut)
                .ok_or("`files.associations` is not a table")?;
            associations.insert(pattern, value(language));
        }
        None => {
            if let Some(associations) = files
                .get_mut("associations")
                .and_then(Item::as_table_like_mut)
            {
                associations.remove(pattern);
            }
        }
    }
    Ok(doc.to_string())
}

/// Applies `edit` to the config file at `path` and writes it back
/// atomically. A missing file starts from the commented default template.
pub fn edit_config_file(
    path: &Path,
    edit: impl FnOnce(&str) -> Result<String, String>,
) -> io::Result<()> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) if err.kind() == io::ErrorKind::NotFound => super::Config::default_toml(),
        Err(err) => return Err(err),
    };
    let edited = edit(&source).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(path, edited.as_bytes())
}

pub(super) fn parse(source: &str) -> Result<DocumentMut, String> {
    source
        .parse::<DocumentMut>()
        .map_err(|err| format!("config.toml is not valid TOML: {}", first_line(&err)))
}

fn first_line(err: &impl std::fmt::Display) -> String {
    err.to_string()
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// The table at `key` in `parent`, created (as an implicit `[a.b]`-style
/// table) when missing. Inline tables are used as they are.
pub(super) fn table_entry<'a>(
    parent: &'a mut dyn TableLike,
    key: &str,
) -> Result<&'a mut dyn TableLike, String> {
    if parent.get(key).is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        parent.insert(key, Item::Table(table));
    }
    parent
        .get_mut(key)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("`{key}` in config.toml is not a table"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    const SOURCE: &str = "# my settings\n[editor]\nfont_size = 15 # big\n\n[language.python]\nservers = [\"pylsp\"]\n";

    #[test]
    fn disabling_and_enabling_keeps_comments() {
        let off = set_languages_enabled(SOURCE, &["python", "Rust"], false).unwrap();
        assert!(off.starts_with("# my settings\n[editor]\nfont_size = 15 # big\n"));
        let (config, warnings) = Config::from_toml_str(&off);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!config.language_enabled("python"));
        assert!(!config.language_enabled("rust"));
        assert!(config.language_enabled("go"));
        let on = set_languages_enabled(&off, &["python", "rust"], true).unwrap();
        let (config, _) = Config::from_toml_str(&on);
        assert!(config.language_enabled("python"));
        assert!(config.language_enabled("rust"));
        assert_eq!(config.language("python").unwrap().servers, ["pylsp"]);
        assert!(on.contains("# big"));
    }

    #[test]
    fn inline_language_tables_are_edited_in_place() {
        let source = "language = { go = { tab_size = 8 } }\n";
        let off = set_languages_enabled(source, &["go"], false).unwrap();
        let (config, warnings) = Config::from_toml_str(&off);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!config.language_enabled("go"));
        assert_eq!(config.language("go").unwrap().tab_size, Some(8));
    }

    #[test]
    fn associations_are_added_and_removed() {
        let with = set_file_association(SOURCE, "*.mdx", Some("markdown")).unwrap();
        let with = set_file_association(&with, "Jenkinsfile", Some("groovy")).unwrap();
        let (config, warnings) = Config::from_toml_str(&with);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.files.associations.len(), 2);
        let without = set_file_association(&with, "*.mdx", None).unwrap();
        let (config, _) = Config::from_toml_str(&without);
        assert_eq!(config.files.associations.len(), 1);
        assert!(without.contains("# my settings"));
    }

    #[test]
    fn invalid_toml_is_refused() {
        assert!(set_languages_enabled("[editor", &["rust"], false).is_err());
    }

    #[test]
    fn a_missing_file_starts_from_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        edit_config_file(&path, |s| set_languages_enabled(s, &["rust"], false)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Arcode configuration"));
        let (config, warnings) = Config::from_toml_str(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!config.language_enabled("rust"));
    }
}
