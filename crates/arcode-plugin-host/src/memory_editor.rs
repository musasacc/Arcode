//! An [`EditorBridge`] over a [`SnapshotMap`] that really applies edits:
//! headless runs, end-to-end tests, and a reference for the app's own
//! implementation (version check first, ranges validated, one step).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ropey::Rope;

use crate::bindings::editor::{EditorError, TextEdit};
use crate::bindings::{DocId, Position};
use crate::bridge::{DocSnapshot, EditorBridge, SnapshotMap};
use crate::cancel::lock;

/// Scratch documents get ids from here up.
const FIRST_SCRATCH_ID: DocId = 1 << 48;

/// What the editor was asked to open.
#[derive(Debug, Clone, PartialEq)]
pub enum Opened {
    /// `open-file`.
    File {
        /// Project-relative path.
        path: String,
        /// Cursor position.
        at: Option<Position>,
    },
    /// `open-scratch`.
    Scratch {
        /// The new document.
        id: DocId,
        /// Its title.
        title: String,
    },
}

/// Documents in memory; edits apply to the snapshots.
#[derive(Debug, Clone)]
pub struct MemoryEditor {
    docs: SnapshotMap,
    focused: Arc<Mutex<Option<DocId>>>,
    opened: Arc<Mutex<Vec<Opened>>>,
    next_scratch: Arc<AtomicU64>,
}

impl Default for MemoryEditor {
    fn default() -> Self {
        Self::new(SnapshotMap::default())
    }
}

impl MemoryEditor {
    /// An editor over `docs`.
    pub fn new(docs: SnapshotMap) -> Self {
        Self {
            docs,
            focused: Arc::default(),
            opened: Arc::default(),
            next_scratch: Arc::new(AtomicU64::new(FIRST_SCRATCH_ID)),
        }
    }

    /// The shared snapshot map.
    pub fn docs(&self) -> &SnapshotMap {
        &self.docs
    }

    /// Publishes (or replaces) a document, as the app does after each edit.
    pub fn publish(&self, snapshot: DocSnapshot) {
        if let Ok(mut docs) = self.docs.write() {
            docs.insert(snapshot.id, snapshot);
        }
    }

    /// Forgets a closed document.
    pub fn close(&self, doc: DocId) {
        if let Ok(mut docs) = self.docs.write() {
            docs.remove(&doc);
        }
    }

    /// Sets the focused document.
    pub fn focus(&self, doc: Option<DocId>) {
        *lock(&self.focused) = doc;
    }

    /// The text of `doc`.
    pub fn text(&self, doc: DocId) -> Option<String> {
        self.snapshot(doc).map(|s| s.rope.to_string())
    }

    /// Everything opened so far.
    pub fn opened(&self) -> Vec<Opened> {
        lock(&self.opened).clone()
    }
}

impl EditorBridge for MemoryEditor {
    fn snapshot(&self, doc: DocId) -> Option<DocSnapshot> {
        self.docs.read().ok()?.get(&doc).cloned()
    }

    fn focused(&self) -> Option<DocId> {
        *lock(&self.focused)
    }

    fn apply_edits(
        &self,
        doc: DocId,
        expected_version: u64,
        edits: Vec<TextEdit>,
    ) -> Result<u64, EditorError> {
        let mut docs = self.docs.write().map_err(|_| EditorError::NoSuchDocument)?;
        let snap = docs.get_mut(&doc).ok_or(EditorError::NoSuchDocument)?;
        if snap.version != expected_version {
            return Err(EditorError::VersionMismatch(snap.version));
        }
        let resolved = snap.resolve_edits(&edits)?;
        let mut rope = snap.rope.clone();
        // Back to front, so earlier ranges stay valid.
        for (range, text) in resolved.iter().rev() {
            rope.remove(range.clone());
            rope.insert(range.start, text);
        }
        snap.rope = rope;
        snap.version += 1;
        Ok(snap.version)
    }

    fn open_file(&self, path: &str, at: Option<Position>) -> Result<(), EditorError> {
        lock(&self.opened).push(Opened::File {
            path: path.to_owned(),
            at,
        });
        Ok(())
    }

    fn open_scratch(&self, title: &str, language: &str, text: &str) -> Result<DocId, EditorError> {
        let id = self.next_scratch.fetch_add(1, Ordering::SeqCst);
        self.publish(DocSnapshot {
            id,
            path: None,
            language: language.to_owned(),
            version: 1,
            rope: Rope::from_str(text),
            selections: Vec::new(),
        });
        lock(&self.opened).push(Opened::Scratch {
            id,
            title: title.to_owned(),
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::Range;

    fn edit(sl: u32, sc: u32, el: u32, ec: u32, text: &str) -> TextEdit {
        let p = |line, column| Position { line, column };
        TextEdit {
            range: Range {
                start: p(sl, sc),
                end: p(el, ec),
            },
            text: text.into(),
        }
    }

    fn editor() -> MemoryEditor {
        let e = MemoryEditor::default();
        e.publish(DocSnapshot {
            id: 1,
            path: Some("a.http".into()),
            language: "http".into(),
            version: 4,
            rope: Rope::from_str("GET /a\nHost: x\n"),
            selections: Vec::new(),
        });
        e
    }

    #[test]
    fn edits_apply_as_one_step_after_a_version_check() {
        let e = editor();
        let edits = vec![edit(1, 6, 1, 7, "example.com"), edit(0, 0, 0, 3, "POST")];
        assert_eq!(
            e.apply_edits(1, 3, edits.clone()),
            Err(EditorError::VersionMismatch(4))
        );
        assert_eq!(e.apply_edits(1, 4, edits), Ok(5));
        assert_eq!(e.text(1).as_deref(), Some("POST /a\nHost: example.com\n"));
        assert_eq!(
            e.apply_edits(1, 4, vec![]),
            Err(EditorError::VersionMismatch(5))
        );
        assert_eq!(
            e.apply_edits(9, 1, vec![]),
            Err(EditorError::NoSuchDocument)
        );
    }

    #[test]
    fn bad_ranges_change_nothing() {
        let e = editor();
        let overlapping = vec![edit(0, 0, 0, 4, "x"), edit(0, 2, 0, 5, "y")];
        assert_eq!(
            e.apply_edits(1, 4, overlapping),
            Err(EditorError::InvalidRange)
        );
        let outside = vec![edit(5, 0, 5, 1, "x")];
        assert_eq!(e.apply_edits(1, 4, outside), Err(EditorError::InvalidRange));
        let backwards = vec![edit(1, 2, 0, 1, "x")];
        assert_eq!(
            e.apply_edits(1, 4, backwards),
            Err(EditorError::InvalidRange)
        );
        assert_eq!(e.text(1).as_deref(), Some("GET /a\nHost: x\n"));
        assert_eq!(e.snapshot(1).map(|s| s.version), Some(4));
    }

    #[test]
    fn scratch_buffers_become_documents() {
        let e = editor();
        let id = e.open_scratch("Body", "json", "{}").unwrap();
        assert_eq!(e.snapshot(id).map(|s| s.language), Some("json".into()));
        e.open_file("src/x.rs", None).unwrap();
        assert_eq!(e.opened().len(), 2);
        e.close(id);
        assert!(e.snapshot(id).is_none());
    }
}
