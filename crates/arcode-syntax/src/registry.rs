//! Language lookup by path or LSP id.

use std::path::Path;

use crate::Language;
use crate::languages::{BUILTIN, PLAIN_TEXT};

/// All known languages, plain text last.
pub fn all_languages() -> &'static [&'static Language] {
    BUILTIN
}

/// The plain-text fallback language (no grammar, no highlighting).
pub fn plain_text() -> &'static Language {
    &PLAIN_TEXT
}

/// Picks the language for a file path.
///
/// Exact file names (e.g. `Cargo.lock`) are checked first, then the
/// extension (case-insensitive). Unknown files are plain text.
pub fn language_for_path(path: &Path) -> &'static Language {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(lang) = BUILTIN.iter().find(|lang| lang.file_names.contains(&name))
    {
        return lang;
    }
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str())
        && let Some(lang) = BUILTIN.iter().find(|lang| {
            lang.extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
    {
        return lang;
    }
    &PLAIN_TEXT
}

/// Looks up a language by its LSP `languageId` (case-insensitive).
pub fn language_by_id(id: &str) -> Option<&'static Language> {
    BUILTIN
        .iter()
        .copied()
        .find(|lang| lang.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_are_unique() {
        let langs = all_languages();
        for (i, a) in langs.iter().enumerate() {
            for b in &langs[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.name, b.name);
            }
        }
    }

    #[test]
    fn plain_text_is_last_and_has_no_grammar() {
        let last = all_languages().last().copied();
        assert_eq!(last, Some(plain_text()));
        assert!(!plain_text().has_grammar());
        assert!(plain_text().highlight_query().is_none());
    }

    #[test]
    fn lookup_by_id_ignores_case() {
        assert_eq!(language_by_id("RUST").map(|l| l.name), Some("Rust"));
        assert!(language_by_id("cobol").is_none());
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(language_for_path(Path::new("A.RS")).id, "rust");
    }
}
