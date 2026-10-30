//! Snippet insertion with tabstop navigation (Tab / Shift+Tab / Escape),
//! built on `arcode_completion::SnippetSession`.

use std::ops::Range;

use arcode_buffer::{Edit, Point, Selection, UndoGroup};
use arcode_completion::{SnippetSession, VariableContext, expand};
use gpui::Context;

use crate::Editor;
use crate::api::map_offset;

impl Editor {
    /// Replaces `range` with the LSP/TextMate `snippet` (one undo step) and
    /// selects its first tabstop. While the snippet is active, Tab and
    /// Shift+Tab move between tabstops, mirrors update as you type, and
    /// Escape, moving the cursor out of the snippet or undoing the insertion
    /// ends it. A tabstop with choices (`${1|a,b|}`) opens a list of them.
    pub fn insert_snippet(&mut self, range: Range<Point>, snippet: &str, cx: &mut Context<Self>) {
        let start = self.point_to_offset(range.start);
        let end = self.point_to_offset(range.end).max(start);
        self.insert_snippet_at(start..end, snippet, Vec::new(), cx);
    }

    /// Replaces the char range `range` with `snippet` and applies `extra`
    /// edits (which must not overlap it, e.g. an auto-import) in the same
    /// undo step, then starts the snippet session.
    pub(crate) fn insert_snippet_at(
        &mut self,
        range: Range<usize>,
        snippet: &str,
        extra: Vec<(Range<usize>, String)>,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let (start, end) = (range.start, range.end);
        let line = self.buffer.char_to_line(start);
        let indent: String = self
            .buffer
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let tab_unit = self.ops_config(cx).indent.text();
        let ctx = VariableContext {
            file_path: self.buffer.path().map(|p| p.to_path_buf()),
            line_index: line,
            selected_text: self.buffer.slice_to_string(start..end),
            current_line: self.buffer.line_text(line),
            ..Default::default()
        };
        let expanded = expand(snippet, &ctx, &indent, &tab_unit);
        // Extra edits before the snippet move where it lands.
        let shift: isize = extra
            .iter()
            .filter(|(r, _)| r.end <= start)
            .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
            .sum();
        let session = SnippetSession::new(&expanded, (start as isize + shift) as usize);
        let before = self.selections.clone();
        let after = session.selections();
        let mut raw = extra;
        raw.push((start..end, expanded.text));
        self.buffer.break_undo_group();
        let edits = self
            .buffer
            .edit_with_selections(raw, UndoGroup::New, &before, &after);
        self.buffer.break_undo_group();
        self.selections = after;
        // The session already has post-insertion ranges: start it only
        // after the insertion itself went through `after_edits`.
        self.snippet = None;
        self.after_edits(edits, cx);
        self.snippet = (!session.is_finished()).then_some(session);
        self.snippet_show_choices(cx);
    }

    /// Whether a snippet session is active.
    pub fn snippet_active(&self) -> bool {
        self.snippet.is_some()
    }

    /// Moves to the next (`forward`) or previous tabstop. Returns `false`
    /// when no snippet is active (so Tab indents as usual).
    pub(crate) fn snippet_jump(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.snippet.as_mut() else {
            return false;
        };
        let next = if forward {
            session.next()
        } else {
            session.prev()
        };
        let finished = session.is_finished();
        if finished {
            self.snippet = None;
        }
        match next {
            Some(sels) => {
                self.buffer.break_undo_group();
                self.set_selections(sels, cx);
                self.snippet_show_choices(cx);
                true
            }
            None => !forward,
        }
    }

    /// Ends the snippet session. Returns whether one was active.
    pub(crate) fn snippet_cancel(&mut self) -> bool {
        self.snippet.take().is_some()
    }

    /// Feeds applied `edits` to the snippet session and applies the mirror
    /// updates it asks for, merged into the same undo step.
    pub(crate) fn snippet_after_edits(&mut self, edits: &[Edit]) -> Vec<Edit> {
        let Some(session) = self.snippet.as_mut() else {
            return Vec::new();
        };
        let mirrors = session.apply_edits(edits);
        if mirrors.is_empty() {
            return Vec::new();
        }
        let mut shifts: Vec<(Range<usize>, usize)> = mirrors
            .iter()
            .map(|(r, t)| (r.clone(), t.chars().count()))
            .collect();
        shifts.sort_by_key(|(r, _)| r.start);
        let before = self.selections.clone();
        let mut after = before.clone();
        after.transform(|s| {
            Selection::new(map_offset(s.anchor, &shifts), map_offset(s.head, &shifts))
        });
        let applied = self
            .buffer
            .edit_with_selections(mirrors, UndoGroup::Merge, &before, &after);
        if let Some(session) = self.snippet.as_mut() {
            session.apply_edits(&applied);
        }
        self.selections = after;
        applied
    }

    /// Ends the snippet once an undo removed its insertion: its text has
    /// collapsed to nothing, so its tabstops are stale and Tab must indent
    /// again.
    pub(crate) fn snippet_after_undo(&mut self) {
        if self.snippet.as_ref().is_some_and(|s| s.extent().is_empty()) {
            self.snippet = None;
        }
    }

    /// Ends the snippet when every cursor left it.
    pub(crate) fn snippet_check_cursor(&mut self) {
        if let Some(session) = &self.snippet
            && !self.selections.iter().any(|s| session.contains(s.head))
        {
            self.snippet = None;
        }
    }
}
