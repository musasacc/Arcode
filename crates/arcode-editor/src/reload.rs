//! Bringing a clean editor in line with its file after the file changed
//! on disk (an external edit, a git checkout, a formatter run).

use std::ops::Range;

use arcode_buffer::{LineEnding, Point};
use gpui::Context;

use crate::Editor;

impl Editor {
    /// Applies `edits` (computed against buffer version `version`) that
    /// turn the text into the file's new content, as one undo step, and
    /// marks the buffer saved with the file's `line_ending`. Cursors move
    /// with the text. Read-only editors (library sources) follow the file
    /// too.
    ///
    /// Does nothing and returns `false` when the buffer has unsaved edits
    /// or its version is no longer `version` (it changed since the edits
    /// were computed); the caller can compute them again.
    pub fn apply_disk_reload(
        &mut self,
        version: u64,
        edits: Vec<(Range<Point>, String)>,
        line_ending: LineEnding,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_dirty() || self.buffer.version() != version {
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
        self.check_dirty(cx);
        cx.notify();
        true
    }
}
