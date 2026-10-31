//! Bringing an editor in line with its file after the file changed on
//! disk (an external edit, a git checkout, a formatter run), and the
//! conflict when it cannot follow: an editor with unsaved edits keeps
//! them, is marked as changed (or deleted) on disk, and does not save over
//! the other program's version without being told to.

use std::io;
use std::ops::Range;

use arcode_buffer::{DiskCheck, DiskStamp, LineEnding, Point};
use gpui::Context;

use crate::{Editor, EditorEvent};

/// How the file on disk differs from what the buffer is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskConflict {
    /// Another program changed the file and the buffer kept its own text.
    Changed,
    /// The file was deleted; the buffer keeps its text (saving writes it
    /// again).
    Deleted,
}

impl Editor {
    /// Applies `edits` (computed against buffer version `version`) that
    /// turn the text into the file's new content, as one undo step, and
    /// marks the buffer saved with the file's `line_ending` and `stamp`.
    /// Cursors move with the text. Read-only editors (library sources)
    /// follow the file too.
    ///
    /// Does nothing and returns `false` when the buffer has unsaved edits
    /// or its version is no longer `version` (it changed since the edits
    /// were computed); the caller can compute them again.
    pub fn apply_disk_reload(
        &mut self,
        version: u64,
        edits: Vec<(Range<Point>, String)>,
        line_ending: LineEnding,
        stamp: Option<DiskStamp>,
        cx: &mut Context<Self>,
    ) -> bool {
        !self.is_dirty() && self.reload_from_disk(version, edits, line_ending, stamp, cx)
    }

    /// Like [`Editor::apply_disk_reload`], but also replaces unsaved edits
    /// (Reload in the changed-on-disk bar). Undo brings them back.
    pub fn discard_and_reload(
        &mut self,
        version: u64,
        edits: Vec<(Range<Point>, String)>,
        line_ending: LineEnding,
        stamp: Option<DiskStamp>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.reload_from_disk(version, edits, line_ending, stamp, cx)
    }

    fn reload_from_disk(
        &mut self,
        version: u64,
        edits: Vec<(Range<Point>, String)>,
        line_ending: LineEnding,
        stamp: Option<DiskStamp>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.buffer.version() != version {
            return false;
        }
        let read_only = std::mem::replace(&mut self.read_only, false);
        let group = self.edit_group.take();
        let autoscroll = self.autoscroll.take();
        self.apply_edits(edits, cx);
        self.edit_group = group;
        self.read_only = read_only;
        // Keep the viewport where it was: the change did not come from the
        // user, so it should not scroll to a cursor that moved with it.
        self.autoscroll = autoscroll;
        self.buffer.set_line_ending(line_ending);
        self.buffer.mark_saved();
        self.buffer.set_disk_stamp(stamp);
        self.set_disk_conflict(None, cx);
        self.check_dirty(cx);
        cx.emit(EditorEvent::Reloaded);
        cx.notify();
        true
    }

    /// Whether the file changed or vanished on disk while the buffer kept
    /// its own text.
    pub fn disk_conflict(&self) -> Option<DiskConflict> {
        self.disk_conflict
    }

    /// Marks (or clears) a difference between the buffer and its file,
    /// emitting [`EditorEvent::DiskConflictChanged`] when it changes.
    pub fn set_disk_conflict(&mut self, conflict: Option<DiskConflict>, cx: &mut Context<Self>) {
        if self.disk_conflict != conflict {
            self.disk_conflict = conflict;
            cx.emit(EditorEvent::DiskConflictChanged);
            cx.notify();
        }
    }

    /// The stamp of the file content the buffer is based on.
    pub fn disk_stamp(&self) -> Option<DiskStamp> {
        self.buffer.disk_stamp()
    }

    /// Keeps the buffer's text over the file's new content (Keep Mine):
    /// `stamp`, what the file holds now, becomes what the buffer is based
    /// on, so the next save writes without asking.
    pub fn keep_mine(&mut self, stamp: Option<DiskStamp>, cx: &mut Context<Self>) {
        self.buffer.set_disk_stamp(stamp);
        if self.disk_conflict == Some(DiskConflict::Changed) {
            self.set_disk_conflict(None, cx);
        }
    }

    /// Saves (formatting first if set up) over whatever the file holds now:
    /// the answer to [`EditorEvent::SaveConflict`].
    pub fn save_overwriting(&mut self, cx: &mut Context<Self>) {
        self.buffer.set_disk_stamp(None);
        self.save_with_format(cx);
    }

    /// Refuses a save over a file whose content is no longer what the
    /// buffer is based on: marks the conflict and emits
    /// [`EditorEvent::SaveConflict`]. A deleted file may be written again.
    pub(crate) fn check_save_conflict(&mut self, cx: &mut Context<Self>) -> io::Result<()> {
        let (Some(path), Some(stamp)) = (self.buffer.path(), self.buffer.disk_stamp()) else {
            return Ok(());
        };
        if stamp.check(path) != DiskCheck::Changed {
            return Ok(());
        }
        self.set_disk_conflict(Some(DiskConflict::Changed), cx);
        cx.emit(EditorEvent::SaveConflict);
        Err(io::Error::other(
            "the file changed on disk since it was opened or saved",
        ))
    }
}
