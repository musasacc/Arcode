//! Text document synchronization: turns editor changes into the
//! `didOpen`/`didChange`/`didSave`/`didClose` parameters a server wants.

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, Range, ServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncSaveOptions, Uri, VersionedTextDocumentIdentifier,
};

/// One edit: replace `range` (in the server's position encoding) with
/// `text`. `range: None` replaces the whole document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    /// Range replaced, or `None` for the whole document.
    pub range: Option<Range>,
    /// Replacement text.
    pub text: String,
}

/// The synchronization behaviour a server asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentSync {
    /// How `didChange` should describe edits.
    pub kind: TextDocumentSyncKind,
    /// Whether `didOpen`/`didClose` are sent.
    pub open_close: bool,
    /// `None`: no `didSave`; `Some(include_text)` otherwise.
    pub save: Option<bool>,
}

impl Default for DocumentSync {
    /// What the specification implies when the server says nothing.
    fn default() -> Self {
        DocumentSync {
            kind: TextDocumentSyncKind::NONE,
            open_close: false,
            save: None,
        }
    }
}

impl DocumentSync {
    /// Reads `textDocumentSync` from the server capabilities. The bare kind
    /// form implies open/close and save notifications (as VS Code does).
    pub fn from_capabilities(caps: &ServerCapabilities) -> Self {
        match &caps.text_document_sync {
            None => DocumentSync::default(),
            Some(TextDocumentSyncCapability::Kind(kind)) => DocumentSync {
                kind: *kind,
                open_close: true,
                save: Some(false),
            },
            Some(TextDocumentSyncCapability::Options(opts)) => DocumentSync {
                kind: opts.change.unwrap_or(TextDocumentSyncKind::NONE),
                open_close: opts.open_close.unwrap_or(false),
                save: match &opts.save {
                    None | Some(TextDocumentSyncSaveOptions::Supported(false)) => None,
                    Some(TextDocumentSyncSaveOptions::Supported(true)) => Some(false),
                    Some(TextDocumentSyncSaveOptions::SaveOptions(o)) => {
                        Some(o.include_text.unwrap_or(false))
                    }
                },
            },
        }
    }

    /// `textDocument/didOpen` params, or `None` if the server does not want them.
    pub fn did_open(
        &self,
        uri: Uri,
        language_id: &str,
        version: i32,
        text: String,
    ) -> Option<DidOpenTextDocumentParams> {
        self.open_close.then(|| DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(uri, language_id.to_string(), version, text),
        })
    }

    /// `textDocument/didChange` params. Full-sync servers get the whole text
    /// (`full_text` is only called then); incremental servers get `changes`.
    /// Returns `None` for servers that do not want change notifications, and
    /// for an incremental change set that is empty.
    pub fn did_change(
        &self,
        uri: Uri,
        version: i32,
        changes: Vec<TextChange>,
        full_text: impl FnOnce() -> String,
    ) -> Option<DidChangeTextDocumentParams> {
        let content_changes = if self.kind == TextDocumentSyncKind::FULL {
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: full_text(),
            }]
        } else if self.kind == TextDocumentSyncKind::INCREMENTAL {
            if changes.is_empty() {
                return None;
            }
            changes
                .into_iter()
                .map(|c| TextDocumentContentChangeEvent {
                    range: c.range,
                    range_length: None,
                    text: c.text,
                })
                .collect()
        } else {
            return None;
        };
        Some(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(uri, version),
            content_changes,
        })
    }

    /// `didChange` params carrying the full text, regardless of sync kind
    /// (used when edit ranges are in a different position encoding).
    pub fn did_change_full(
        &self,
        uri: Uri,
        version: i32,
        text: String,
    ) -> Option<DidChangeTextDocumentParams> {
        if self.kind == TextDocumentSyncKind::NONE {
            return None;
        }
        Some(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(uri, version),
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            }],
        })
    }

    /// `textDocument/didSave` params; `text` is only called when the server
    /// asked for `includeText`.
    pub fn did_save(
        &self,
        uri: Uri,
        text: impl FnOnce() -> String,
    ) -> Option<DidSaveTextDocumentParams> {
        let include_text = self.save?;
        Some(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri),
            text: include_text.then(text),
        })
    }

    /// `textDocument/didClose` params, or `None` if the server does not want them.
    pub fn did_close(&self, uri: Uri) -> Option<DidCloseTextDocumentParams> {
        self.open_close.then(|| DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, SaveOptions, TextDocumentSyncOptions};
    use std::str::FromStr;

    fn uri() -> Uri {
        Uri::from_str("file:///a.rs").unwrap()
    }

    fn caps(sync: Option<TextDocumentSyncCapability>) -> ServerCapabilities {
        ServerCapabilities {
            text_document_sync: sync,
            ..Default::default()
        }
    }

    fn change() -> TextChange {
        TextChange {
            range: Some(Range::new(Position::new(0, 1), Position::new(0, 2))),
            text: "x".into(),
        }
    }

    #[test]
    fn incremental_sends_ranges_and_never_builds_full_text() {
        let s =
            DocumentSync::from_capabilities(&caps(Some(TextDocumentSyncKind::INCREMENTAL.into())));
        let p = s
            .did_change(uri(), 2, vec![change()], || panic!("full text not needed"))
            .unwrap();
        assert_eq!(p.text_document.version, 2);
        assert_eq!(p.content_changes.len(), 1);
        assert_eq!(p.content_changes[0].range, change().range);
        assert!(s.did_change(uri(), 3, vec![], || unreachable!()).is_none());
    }

    #[test]
    fn full_sends_whole_text() {
        let s = DocumentSync::from_capabilities(&caps(Some(TextDocumentSyncKind::FULL.into())));
        let p = s
            .did_change(uri(), 5, vec![change()], || "all".into())
            .unwrap();
        assert_eq!(p.content_changes.len(), 1);
        assert_eq!(p.content_changes[0].range, None);
        assert_eq!(p.content_changes[0].text, "all");
    }

    #[test]
    fn none_sends_nothing() {
        let s = DocumentSync::from_capabilities(&caps(None));
        assert!(s.did_open(uri(), "rust", 1, "t".into()).is_none());
        assert!(
            s.did_change(uri(), 1, vec![change()], || "t".into())
                .is_none()
        );
        assert!(s.did_change_full(uri(), 1, "t".into()).is_none());
        assert!(s.did_save(uri(), || "t".into()).is_none());
        assert!(s.did_close(uri()).is_none());
    }

    #[test]
    fn options_form_is_respected() {
        let opts = TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::INCREMENTAL),
            save: Some(
                SaveOptions {
                    include_text: Some(true),
                }
                .into(),
            ),
            ..Default::default()
        };
        let s = DocumentSync::from_capabilities(&caps(Some(opts.into())));
        let open = s.did_open(uri(), "rust", 1, "fn main() {}".into()).unwrap();
        assert_eq!(open.text_document.language_id, "rust");
        assert_eq!(open.text_document.version, 1);
        assert_eq!(
            s.did_save(uri(), || "saved".into())
                .unwrap()
                .text
                .as_deref(),
            Some("saved")
        );
        assert!(s.did_close(uri()).is_some());
        let full = s.did_change_full(uri(), 9, "z".into()).unwrap();
        assert_eq!(full.content_changes[0].range, None);
    }

    #[test]
    fn save_without_text() {
        let opts = TextDocumentSyncOptions {
            save: Some(true.into()),
            ..Default::default()
        };
        let s = DocumentSync::from_capabilities(&caps(Some(opts.into())));
        let p = s.did_save(uri(), || panic!("text not requested")).unwrap();
        assert_eq!(p.text, None);
        assert!(!s.open_close);
    }
}
