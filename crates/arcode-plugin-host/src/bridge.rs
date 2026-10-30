//! What the host needs from the app, as traits (§5, §12 `bridge.rs`).
//!
//! The app implements these on top of its editor, file index and network
//! stack; tests use fakes. All methods are called on plugin threads.
//!
//! - Editor reads never hop to the UI thread: the app publishes a
//!   [`DocSnapshot`] per open document into a [`SnapshotMap`].
//! - Editor writes (`apply-edits`, `open-file`, `open-scratch`) are messages
//!   to the UI thread; the implementation waits at most [`UI_REPLY_TIMEOUT`].

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use ropey::Rope;

use crate::bindings::editor::{EditorError, TextEdit};
use crate::bindings::http::{HttpError, Request, Response};
use crate::bindings::{DocId, Position, Range};
use crate::cancel::CancelToken;

/// How long a plugin thread waits for the UI thread to apply an edit or
/// open a document.
pub const UI_REPLY_TIMEOUT: Duration = Duration::from_secs(1);

/// An immutable view of one open document (cloning a `Rope` is O(1)).
#[derive(Debug, Clone)]
pub struct DocSnapshot {
    /// Editor document id.
    pub id: DocId,
    /// Project-relative path; `None` for untitled buffers.
    pub path: Option<String>,
    /// Language id.
    pub language: String,
    /// Bumped on every edit.
    pub version: u64,
    /// The text.
    pub rope: Rope,
    /// Selections, zero-based, columns in chars.
    pub selections: Vec<Range>,
}

impl DocSnapshot {
    /// Char index of `p`, if it lies inside the document (a column may be
    /// at most the line's length, newline included).
    pub fn char_index(&self, p: Position) -> Option<usize> {
        let line = p.line as usize;
        if line >= self.rope.len_lines() {
            return None;
        }
        let line_len = self.rope.line(line).len_chars();
        let column = p.column as usize;
        (column <= line_len).then(|| self.rope.line_to_char(line) + column)
    }

    /// Char range of `r`, if both ends are inside and `start <= end`.
    pub fn char_range(&self, r: Range) -> Option<std::ops::Range<usize>> {
        let (start, end) = (self.char_index(r.start)?, self.char_index(r.end)?);
        (start <= end).then_some(start..end)
    }

    /// Resolves `edits` against this snapshot: char ranges sorted by start,
    /// refused with [`EditorError::InvalidRange`] if any range is outside
    /// the document or two ranges overlap.
    pub fn resolve_edits<'a>(
        &self,
        edits: &'a [TextEdit],
    ) -> Result<Vec<(std::ops::Range<usize>, &'a str)>, EditorError> {
        let mut resolved = edits
            .iter()
            .map(|e| Some((self.char_range(e.range)?, e.text.as_str())))
            .collect::<Option<Vec<_>>>()
            .ok_or(EditorError::InvalidRange)?;
        resolved.sort_by_key(|(r, _)| (r.start, r.end));
        if resolved.windows(2).any(|w| w[0].0.end > w[1].0.start) {
            return Err(EditorError::InvalidRange);
        }
        Ok(resolved)
    }
}

/// Snapshots of every open document, published by the app after each edit
/// while any plugin is active.
pub type SnapshotMap = Arc<RwLock<HashMap<DocId, DocSnapshot>>>;

/// The editor side of the `editor` interface.
pub trait EditorBridge: Send + Sync {
    /// The latest snapshot of `doc`.
    fn snapshot(&self, doc: DocId) -> Option<DocSnapshot>;
    /// The focused document, if any.
    fn focused(&self) -> Option<DocId>;
    /// Applies `edits` as one undo step if `doc` is still at
    /// `expected_version`; returns the new version.
    fn apply_edits(
        &self,
        doc: DocId,
        expected_version: u64,
        edits: Vec<TextEdit>,
    ) -> Result<u64, EditorError>;
    /// Opens a project-relative path (already validated by the host).
    fn open_file(&self, path: &str, at: Option<Position>) -> Result<(), EditorError>;
    /// Opens an untitled, unsaved buffer.
    fn open_scratch(&self, title: &str, language: &str, text: &str) -> Result<DocId, EditorError>;
}

/// The project's file index (honours `.gitignore`).
pub trait ProjectIndex: Send + Sync {
    /// Project-relative paths (with `/`) matching `matcher`, at most `limit`.
    fn find_files(&self, matcher: &globset::GlobMatcher, limit: usize) -> Vec<String>;
}

/// Does the actual HTTP exchange for `http.send` once the host has checked
/// gesture, master switch and consent. The real implementation is
/// [`crate::net::UreqTransport`].
pub trait HttpTransport: Send + Sync {
    /// Sends `request`. `allow_redirect` is called with the `(host, port)`
    /// of every redirect target before following it and returns false to
    /// stop (consent is re-checked per hop). Must return
    /// [`HttpError::Cancelled`] promptly once `cancel` fires.
    fn send(
        &self,
        request: Request,
        cancel: &CancelToken,
        allow_redirect: &mut dyn FnMut(&str, u16) -> bool,
    ) -> Result<Response, HttpError>;
}

/// An editor with no documents (headless use, tests).
#[derive(Debug, Default, Clone)]
pub struct NoEditor;

impl EditorBridge for NoEditor {
    fn snapshot(&self, _doc: DocId) -> Option<DocSnapshot> {
        None
    }
    fn focused(&self) -> Option<DocId> {
        None
    }
    fn apply_edits(&self, _: DocId, _: u64, _: Vec<TextEdit>) -> Result<u64, EditorError> {
        Err(EditorError::NoSuchDocument)
    }
    fn open_file(&self, _: &str, _: Option<Position>) -> Result<(), EditorError> {
        Err(EditorError::Denied)
    }
    fn open_scratch(&self, _: &str, _: &str, _: &str) -> Result<DocId, EditorError> {
        Err(EditorError::Denied)
    }
}

/// An editor backed only by a [`SnapshotMap`]; writes are refused.
#[derive(Debug, Default, Clone)]
pub struct SnapshotEditor {
    /// The published snapshots.
    pub docs: SnapshotMap,
    /// The focused document.
    pub focused: Option<DocId>,
}

impl EditorBridge for SnapshotEditor {
    fn snapshot(&self, doc: DocId) -> Option<DocSnapshot> {
        let docs = self.docs.read().ok()?;
        docs.get(&doc).cloned()
    }
    fn focused(&self) -> Option<DocId> {
        self.focused
    }
    fn apply_edits(&self, _: DocId, _: u64, _: Vec<TextEdit>) -> Result<u64, EditorError> {
        Err(EditorError::Denied)
    }
    fn open_file(&self, _: &str, _: Option<Position>) -> Result<(), EditorError> {
        Err(EditorError::Denied)
    }
    fn open_scratch(&self, _: &str, _: &str, _: &str) -> Result<DocId, EditorError> {
        Err(EditorError::Denied)
    }
}

/// A project without an index: `find-files` answers nothing.
#[derive(Debug, Default, Clone)]
pub struct NoIndex;

impl ProjectIndex for NoIndex {
    fn find_files(&self, _: &globset::GlobMatcher, _: usize) -> Vec<String> {
        Vec::new()
    }
}

/// No network stack: every request fails (headless use, tests).
#[derive(Debug, Default, Clone)]
pub struct NoHttp;

impl HttpTransport for NoHttp {
    fn send(
        &self,
        _: Request,
        _: &CancelToken,
        _: &mut dyn FnMut(&str, u16) -> bool,
    ) -> Result<Response, HttpError> {
        Err(HttpError::Io("no HTTP transport configured".into()))
    }
}
