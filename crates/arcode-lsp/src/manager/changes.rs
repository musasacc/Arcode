//! Document changes, saves and closes, sent to every server of a file.

use std::path::Path;
use std::sync::Arc;

use super::actor::Command;
use super::text::LazyText;
use super::{LspManager, absolute};
use crate::event::PositionEncoding;
use crate::sync::TextChange;

impl LspManager {
    /// Reports edits. Ranges are UTF-16 (the LSP default); servers that
    /// negotiated another encoding receive `full_text` instead.
    ///
    /// Prefer [`LspManager::change_document_lazy`] for large documents: it
    /// only builds the full text if some server needs it.
    pub fn change_document(
        &self,
        path: &Path,
        version: i32,
        changes: Vec<TextChange>,
        full_text: impl Into<Arc<str>>,
    ) {
        self.change_document_encoded(path, version, changes, PositionEncoding::Utf16, full_text);
    }

    /// Reports edits whose ranges use `encoding`. Servers using that encoding
    /// get incremental changes if they want them; others get `full_text`.
    pub fn change_document_encoded(
        &self,
        path: &Path,
        version: i32,
        changes: Vec<TextChange>,
        encoding: PositionEncoding,
        full_text: impl Into<Arc<str>>,
    ) {
        let text = LazyText::ready(full_text.into());
        self.send_change(path, version, Arc::new([(encoding, changes)]), text);
    }

    /// Like [`LspManager::change_document_encoded`], but `full_text` is only
    /// called (at most once, on a runtime thread, possibly later) if a server
    /// needs the whole document: full sync, a different position encoding,
    /// an empty `changes` list, or a later `didSave` with text. Servers that
    /// sync incrementally never trigger it, so typing costs no O(n) copy.
    ///
    /// `full_text` must capture a snapshot of the text at `version` (e.g. a
    /// cloned rope), not a reference to the live buffer.
    pub fn change_document_lazy(
        &self,
        path: &Path,
        version: i32,
        changes: Vec<TextChange>,
        encoding: PositionEncoding,
        full_text: impl FnOnce() -> String + Send + 'static,
    ) {
        let text = LazyText::deferred(full_text);
        self.send_change(path, version, Arc::new([(encoding, changes)]), text);
    }

    /// Like [`LspManager::change_document_lazy`] for a document served by
    /// servers with different position encodings: `changes` is called now
    /// once for each encoding the document's servers use, so every server
    /// gets incremental changes in its own encoding (a server that has not
    /// finished starting is assumed to use UTF-16, the LSP default).
    pub fn change_document_multi(
        &self,
        path: &Path,
        version: i32,
        changes: impl Fn(PositionEncoding) -> Vec<TextChange>,
        full_text: impl FnOnce() -> String + Send + 'static,
    ) {
        let encodings = self.document_encodings(path);
        let per_encoding: Vec<_> = encodings.into_iter().map(|e| (e, changes(e))).collect();
        let text = LazyText::deferred(full_text);
        self.send_change(path, version, per_encoding.into(), text);
    }

    /// The distinct position encodings of `path`'s servers.
    fn document_encodings(&self, path: &Path) -> Vec<PositionEncoding> {
        let state = self.lock();
        let mut out = Vec::new();
        for slot in state
            .documents
            .get(&absolute(path))
            .map_or(&[][..], |d| &d.servers)
        {
            let encoding = slot
                .running_client()
                .map_or(PositionEncoding::Utf16, |c| c.position_encoding());
            if !out.contains(&encoding) {
                out.push(encoding);
            }
        }
        out
    }

    fn send_change(
        &self,
        path: &Path,
        version: i32,
        changes: Arc<[(PositionEncoding, Vec<TextChange>)]>,
        full_text: Arc<LazyText>,
    ) {
        let mut state = self.lock();
        let Some(doc) = state.documents.get_mut(&absolute(path)) else {
            return;
        };
        doc.text = full_text.clone();
        doc.version = version;
        let latest = full_text.clone();
        doc.open_text = Arc::new(move || latest.get().to_string());
        for slot in &doc.servers {
            slot.send(Command::Change {
                uri: doc.uri.clone(),
                version,
                changes: changes.clone(),
                full_text: full_text.clone(),
            });
        }
    }

    /// Sends `didSave` (with the latest text if a server asked for it).
    pub fn save_document(&self, path: &Path) {
        let state = self.lock();
        let Some(doc) = state.documents.get(&absolute(path)) else {
            return;
        };
        for slot in &doc.servers {
            slot.send(Command::Save {
                uri: doc.uri.clone(),
                text: doc.text.clone(),
            });
        }
    }

    /// Sends `didClose` and forgets the document.
    pub fn close_document(&self, path: &Path) {
        let Some(doc) = self.lock().documents.remove(&absolute(path)) else {
            return;
        };
        for slot in &doc.servers {
            slot.send(Command::Close {
                uri: doc.uri.clone(),
            });
        }
    }
}
