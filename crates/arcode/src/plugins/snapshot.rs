//! Document snapshots for plugins (docs/PLUGINS.md §5): what an open
//! editor looks like to `editor.text`, `editor.selections` and
//! `provide-code-lenses`. Built on the UI thread after edits; cloning the
//! rope is O(1).

use std::path::Path;

use arcode_editor::Editor;
use arcode_plugin_host::bindings::{DocId, Position, Range};
use arcode_plugin_host::bridge::DocSnapshot;
use gpui::{App, Entity};

/// The plugin-facing id of an editor's document.
pub fn doc_id(editor: &Entity<Editor>) -> DocId {
    editor.entity_id().as_u64()
}

/// A file extension a plugin contributes a language for (`.http` →
/// `http`), lower-case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedLanguage {
    /// Extension without the dot, lower-case.
    pub extension: String,
    /// Language id.
    pub id: String,
    /// Display name.
    pub name: String,
}

/// Extensions (space separated) no plugin language may claim, besides
/// plain text's own (`.txt`): files that usually hold keys, certificates,
/// credentials, configuration or private notes. Plain text stays private (the host gives
/// `editor = "context"` plugins documents of languages they contribute, and
/// the app never asks lens providers about plain text), so claiming these
/// would hand them to a plugin without a prompt.
const NEVER_CLAIMED: &str = "asc bak cer cfg conf cred credentials crt csr csv der env gpg \
    htpasswd ini jks kdb kdbx key keystore log netrc note notes old orig ovpn p12 p7b p8 pass \
    passwd password pem pfx pgp ppk properties pub secret secrets sig text tmp token tsv";

/// Whether a plugin language may claim files with extension `ext`: not one
/// of plain text's own extensions nor one of [`NEVER_CLAIMED`].
pub fn claimable(ext: &str) -> bool {
    let plain = arcode_syntax::plain_text().extensions;
    !plain
        .iter()
        .copied()
        .chain(NEVER_CLAIMED.split_whitespace())
        .any(|e| e.eq_ignore_ascii_case(ext))
}

/// The language id plugins see for a document: the editor's own language,
/// unless the editor has nothing better than plain text and a plugin
/// contributes a language for the file's extension (Arcode has no `.http`
/// grammar, the HTTP client contributes `http`), and that extension is
/// [`claimable`].
pub fn language_for(
    path: Option<&Path>,
    editor_language: &str,
    contributed: &[ContributedLanguage],
) -> String {
    let plain = arcode_syntax::plain_text().id;
    if editor_language == plain
        && let Some(ext) = path.and_then(|p| p.extension()).and_then(|e| e.to_str())
        && claimable(ext)
        && let Some(lang) = contributed
            .iter()
            .find(|l| l.extension.eq_ignore_ascii_case(ext))
    {
        return lang.id.clone();
    }
    editor_language.to_owned()
}

/// `path` relative to `root` with `/` separators; `None` outside the
/// project (plugins only see project files).
pub fn relative_path(root: Option<&Path>, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root?).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn position(editor: &Editor, offset: usize) -> Position {
    let p = editor.offset_to_point(offset);
    Position {
        line: u32::try_from(p.line).unwrap_or(u32::MAX),
        column: u32::try_from(p.column).unwrap_or(u32::MAX),
    }
}

/// The snapshot of `editor` now.
pub fn snapshot(
    editor: &Entity<Editor>,
    root: Option<&Path>,
    contributed: &[ContributedLanguage],
    cx: &App,
) -> DocSnapshot {
    let e = editor.read(cx);
    let path = e.path();
    let selections = e
        .selections()
        .iter()
        .map(|s| Range {
            start: position(e, s.start()),
            end: position(e, s.end()),
        })
        .collect();
    DocSnapshot {
        id: doc_id(editor),
        path: path.and_then(|p| relative_path(root, p)),
        language: language_for(path, e.language().id, contributed),
        version: e.buffer().version(),
        rope: e.buffer().text().clone(),
        selections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http() -> Vec<ContributedLanguage> {
        vec![ContributedLanguage {
            extension: "http".into(),
            id: "http".into(),
            name: "HTTP Request".into(),
        }]
    }

    #[test]
    fn plugin_languages_only_replace_plain_text() {
        let plain = arcode_syntax::plain_text().id;
        assert_eq!(
            language_for(Some(Path::new("/p/a.HTTP")), plain, &http()),
            "http"
        );
        assert_eq!(
            language_for(Some(Path::new("/p/a.http")), "rust", &http()),
            "rust"
        );
        assert_eq!(
            language_for(Some(Path::new("/p/a.txt")), plain, &http()),
            plain
        );
        assert_eq!(language_for(None, plain, &http()), plain);
    }

    #[test]
    fn private_plain_text_cannot_be_claimed() {
        assert!(claimable("http") && claimable("REST"));
        for ext in ["txt", "TXT", "text", "pem", "key", "env", "log", "kdbx"] {
            assert!(!claimable(ext), "{ext}");
        }
        let plain = arcode_syntax::plain_text().id;
        let env = vec![ContributedLanguage {
            extension: "env".into(),
            id: "dotenv".into(),
            name: "Env".into(),
        }];
        assert_eq!(
            language_for(Some(Path::new("/p/prod.env")), plain, &env),
            plain
        );
    }

    #[test]
    fn relative_paths_use_slashes_and_stay_inside() {
        let root = Path::new("/p");
        assert_eq!(
            relative_path(Some(root), Path::new("/p/a/b.http")).as_deref(),
            Some("a/b.http")
        );
        assert_eq!(relative_path(Some(root), Path::new("/q/b.http")), None);
        assert_eq!(relative_path(None, Path::new("/p/b.http")), None);
        assert_eq!(relative_path(Some(root), root), None);
    }
}
