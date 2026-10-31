//! The app's side of the host traits (docs/PLUGINS.md §5 "Editor reads
//! never hop to the UI thread"): [`AppEditor`] implements
//! [`EditorBridge`] and [`AppIndex`] implements [`ProjectIndex`].
//!
//! Both are called on plugin threads. Reads come from data the UI thread
//! published (document snapshots, the project's file list); writes
//! (`apply-edits`, `open-file`, `open-scratch`) become [`EditorRequest`]s
//! that a workspace task applies on the UI thread. The plugin thread waits
//! at most [`UI_REPLY_TIMEOUT`] for the answer; the UI thread never waits
//! on a plugin.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, OnceLock, RwLock};

use arcode_plugin_host::bindings::editor::{EditorError, TextEdit};
use arcode_plugin_host::bindings::http::{HttpError, Request, Response};
use arcode_plugin_host::bindings::{DocId, Position};
use arcode_plugin_host::bridge::{
    DocSnapshot, EditorBridge, HttpTransport, ProjectIndex, SnapshotMap, UI_REPLY_TIMEOUT,
};
use arcode_plugin_host::cancel::CancelToken;
use arcode_plugin_host::index::WalkIndex;
use arcode_plugin_host::net::UreqTransport;

/// Where an [`EditorRequest`]'s answer goes (the plugin thread waits on
/// the other end).
pub type Reply<T> = SyncSender<Result<T, EditorError>>;

/// An editor write a plugin asked for, applied on the UI thread.
#[derive(Debug)]
pub enum EditorRequest {
    /// `editor.apply-edits`: one undo step if the document is still at
    /// `expected` (the host already checked ranges and consent).
    ApplyEdits {
        /// Target document.
        doc: DocId,
        /// The version the edits were computed against.
        expected: u64,
        /// Non-overlapping edits.
        edits: Vec<TextEdit>,
        /// New version, or why not.
        reply: Reply<u64>,
    },
    /// `editor.open-file` (project-relative, validated by the host).
    OpenFile {
        /// Project-relative path with `/`.
        path: String,
        /// Where to put the cursor.
        at: Option<Position>,
        /// Done, or why not.
        reply: Reply<()>,
    },
    /// `editor.open-scratch`: an untitled buffer.
    OpenScratch {
        /// Tab title.
        title: String,
        /// Language id (syntax highlighting).
        language: String,
        /// Initial text.
        text: String,
        /// The new document's id.
        reply: Reply<DocId>,
    },
}

/// The editor as plugins see it.
#[derive(Debug)]
pub struct AppEditor {
    docs: SnapshotMap,
    /// Doc id of the focused editor, 0 for none.
    focused: AtomicU64,
    requests: async_channel::Sender<EditorRequest>,
}

impl AppEditor {
    /// A bridge whose writes go to `requests`.
    pub fn new(requests: async_channel::Sender<EditorRequest>) -> Self {
        Self {
            docs: SnapshotMap::default(),
            focused: AtomicU64::new(0),
            requests,
        }
    }

    /// Publishes (or replaces) a document's snapshot.
    pub fn publish(&self, snapshot: DocSnapshot) {
        if let Ok(mut docs) = self.docs.write() {
            docs.insert(snapshot.id, snapshot);
        }
    }

    /// Forgets a closed document.
    pub fn forget(&self, doc: DocId) {
        if let Ok(mut docs) = self.docs.write() {
            docs.remove(&doc);
        }
        let _ = self
            .focused
            .compare_exchange(doc, 0, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Sets the focused document.
    pub fn set_focused(&self, doc: Option<DocId>) {
        self.focused.store(doc.unwrap_or(0), Ordering::SeqCst);
    }

    /// Whether `doc` has a published snapshot.
    pub fn has(&self, doc: DocId) -> bool {
        self.docs.read().is_ok_and(|d| d.contains_key(&doc))
    }

    /// Sends `make(reply)` to the UI thread and waits for the answer.
    fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> EditorRequest) -> Result<T, EditorError> {
        let (tx, rx) = sync_channel(1);
        if self.requests.try_send(make(tx)).is_err() {
            return Err(EditorError::Denied);
        }
        rx.recv_timeout(UI_REPLY_TIMEOUT)
            .unwrap_or(Err(EditorError::Denied))
    }
}

impl EditorBridge for AppEditor {
    fn snapshot(&self, doc: DocId) -> Option<DocSnapshot> {
        self.docs.read().ok()?.get(&doc).cloned()
    }

    fn focused(&self) -> Option<DocId> {
        Some(self.focused.load(Ordering::SeqCst)).filter(|d| *d != 0)
    }

    fn apply_edits(
        &self,
        doc: DocId,
        expected: u64,
        edits: Vec<TextEdit>,
    ) -> Result<u64, EditorError> {
        self.ask(|reply| EditorRequest::ApplyEdits {
            doc,
            expected,
            edits,
            reply,
        })
    }

    fn open_file(&self, path: &str, at: Option<Position>) -> Result<(), EditorError> {
        let path = path.to_owned();
        self.ask(|reply| EditorRequest::OpenFile { path, at, reply })
    }

    fn open_scratch(&self, title: &str, language: &str, text: &str) -> Result<DocId, EditorError> {
        let (title, language, text) = (title.to_owned(), language.to_owned(), text.to_owned());
        self.ask(|reply| EditorRequest::OpenScratch {
            title,
            language,
            text,
            reply,
        })
    }
}

/// The network stack, built on the first request a plugin sends (the TLS
/// configuration and root certificates are not loaded before that).
#[derive(Default)]
pub struct LazyHttp {
    transport: OnceLock<UreqTransport>,
}

impl HttpTransport for LazyHttp {
    fn send(
        &self,
        request: Request,
        cancel: &CancelToken,
        allow_redirect: &mut dyn FnMut(&str, &str) -> bool,
    ) -> Result<Response, HttpError> {
        self.transport
            .get_or_init(UreqTransport::default)
            .send(request, cancel, allow_redirect)
    }
}

#[derive(Debug, Default, Clone)]
struct IndexState {
    root: Option<PathBuf>,
    files: Arc<Vec<String>>,
}

/// `workspace.find-files` answered from the workspace's file list (the
/// finder's `.gitignore`-aware index). While that list is not loaded yet,
/// the project is walked on demand.
#[derive(Debug, Default)]
pub struct AppIndex {
    state: RwLock<IndexState>,
}

impl AppIndex {
    /// Points the index at a project (and drops the old list).
    pub fn set_root(&self, root: Option<PathBuf>) {
        if let Ok(mut s) = self.state.write() {
            *s = IndexState {
                root,
                files: Arc::default(),
            };
        }
    }

    /// The project's files, relative with `/` (from the finder's scan).
    pub fn set_files(&self, files: Arc<Vec<String>>) {
        if let Ok(mut s) = self.state.write() {
            s.files = files;
        }
    }
}

impl ProjectIndex for AppIndex {
    fn find_files(&self, matcher: &globset::GlobMatcher, limit: usize) -> Vec<String> {
        let state = self.state.read().map(|s| s.clone()).unwrap_or_default();
        if state.files.is_empty() {
            return state
                .root
                .map(|root| WalkIndex::new(&root).find_files(matcher, limit))
                .unwrap_or_default();
        }
        state
            .files
            .iter()
            .filter(|f| matcher.is_match(f.as_str()))
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: DocId, text: &str) -> DocSnapshot {
        DocSnapshot {
            id,
            path: Some("a.http".into()),
            language: "http".into(),
            version: 3,
            rope: text.into(),
            selections: Vec::new(),
        }
    }

    #[test]
    fn snapshots_and_focus() {
        let (tx, _rx) = async_channel::unbounded();
        let editor = AppEditor::new(tx);
        assert!(editor.snapshot(7).is_none());
        editor.publish(snapshot(7, "GET /\n"));
        editor.set_focused(Some(7));
        assert_eq!(editor.focused(), Some(7));
        assert_eq!(editor.snapshot(7).unwrap().rope.to_string(), "GET /\n");
        editor.forget(7);
        assert!(editor.snapshot(7).is_none());
        assert_eq!(editor.focused(), None);
    }

    #[test]
    fn writes_are_answered_by_the_ui_side() {
        let (tx, rx) = async_channel::unbounded();
        let editor = Arc::new(AppEditor::new(tx));
        let ui = std::thread::spawn(move || {
            match rx.recv_blocking().unwrap() {
                EditorRequest::ApplyEdits {
                    doc,
                    expected,
                    reply,
                    ..
                } => {
                    assert_eq!((doc, expected), (1, 3));
                    reply.send(Ok(4)).unwrap();
                }
                other => panic!("unexpected {other:?}"),
            }
            // The second request is never answered: the plugin times out.
            let _kept = rx.recv_blocking().unwrap();
            std::thread::sleep(UI_REPLY_TIMEOUT + std::time::Duration::from_millis(200));
        });
        assert_eq!(editor.apply_edits(1, 3, Vec::new()), Ok(4));
        let started = std::time::Instant::now();
        assert_eq!(editor.open_file("a.txt", None), Err(EditorError::Denied));
        assert!(started.elapsed() >= UI_REPLY_TIMEOUT);
        ui.join().unwrap();
    }

    #[test]
    fn closed_ui_channel_refuses_at_once() {
        let (tx, rx) = async_channel::unbounded();
        drop(rx);
        let editor = AppEditor::new(tx);
        assert_eq!(
            editor.open_scratch("t", "json", "{}"),
            Err(EditorError::Denied)
        );
    }

    #[test]
    fn index_uses_the_file_list_then_walks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.db"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        let index = AppIndex::default();
        let m = globset::Glob::new("**/*.db").unwrap().compile_matcher();
        assert!(index.find_files(&m, 10).is_empty());
        index.set_root(Some(dir.path().to_path_buf()));
        assert_eq!(index.find_files(&m, 10), ["a.db"]);
        index.set_files(Arc::new(vec![
            "x/y.db".into(),
            "z.db".into(),
            "q.rs".into(),
        ]));
        assert_eq!(index.find_files(&m, 10), ["x/y.db", "z.db"]);
        assert_eq!(index.find_files(&m, 1), ["x/y.db"]);
    }
}
