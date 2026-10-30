//! Text-changing actions, undo/redo, clipboard and save.

use std::ops::Range;

use arcode_buffer::{Buffer, Selection, SelectionSet, UndoGroup, ops};
use gpui::{ClipboardItem, Context, Window};

use crate::Editor;
use crate::actions::*;

/// Whole-line ranges (with line break) covering the lines of `selections`,
/// merged and sorted.
fn line_ranges(buffer: &Buffer, selections: &SelectionSet) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for s in selections {
        let line = buffer.char_to_line(s.head);
        let range = buffer.line_start(line)..buffer.line_start(line + 1);
        match out.last_mut() {
            Some(last) if last.end >= range.start => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}

impl Editor {
    pub(crate) fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_with(cx, ops::backspace);
    }
    pub(crate) fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_with(cx, |b, s, _| ops::delete_forward(b, s));
    }
    pub(crate) fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_with(cx, |b, s, _| ops::delete_word_left(b, s));
    }
    pub(crate) fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_with(cx, |b, s, _| ops::delete_word_right(b, s));
    }
    pub(crate) fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_with(cx, ops::newline_and_indent);
    }
    pub(crate) fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.snippet_jump(true, cx) {
            return;
        }
        self.edit_with(cx, ops::insert_tab);
    }
    pub(crate) fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        if self.snippet_active() && self.snippet_jump(false, cx) {
            return;
        }
        self.edit_with(cx, |b, s, cfg| {
            ops::outdent_lines(b, s, cfg.indent, cfg.tab_width)
        });
    }
    pub(crate) fn toggle_line_comment(
        &mut self,
        _: &ToggleLineComment,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prefix) = self.language.line_comment else {
            return;
        };
        self.edit_with(cx, |b, s, _| ops::toggle_line_comment(b, s, prefix));
    }
    pub(crate) fn move_line_up(&mut self, _: &MoveLineUp, _: &mut Window, cx: &mut Context<Self>) {
        self.edit_with(cx, |b, s, _| ops::move_lines_up(b, s));
    }
    pub(crate) fn move_line_down(
        &mut self,
        _: &MoveLineDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_with(cx, |b, s, _| ops::move_lines_down(b, s));
    }
    pub(crate) fn duplicate_line(
        &mut self,
        _: &DuplicateLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_with(cx, |b, s, _| ops::duplicate_line(b, s));
    }

    /// Undoes the last change (public for app menus and tests).
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(result) = self.buffer.undo() {
            self.selections = result.selections;
            self.after_edits(result.edits, cx);
            self.snippet_after_undo();
        }
    }

    /// Redoes the last undone change.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(result) = self.buffer.redo() {
            self.selections = result.selections;
            self.after_edits(result.edits, cx);
        }
    }

    pub(crate) fn undo_action(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.undo(cx);
    }
    pub(crate) fn redo_action(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.redo(cx);
    }

    /// The text copy/cut put on the clipboard and whether it is whole lines.
    fn clipboard_text(&self) -> (String, bool) {
        if self.selections.iter().all(Selection::is_empty) {
            let text = line_ranges(&self.buffer, &self.selections)
                .into_iter()
                .map(|r| {
                    let mut t = self.buffer.slice_to_string(r);
                    if !t.ends_with('\n') {
                        t.push('\n');
                    }
                    t
                })
                .collect();
            (text, true)
        } else {
            (ops::selected_text(&self.buffer, &self.selections), false)
        }
    }

    pub(crate) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let (text, lines) = self.clipboard_text();
        self.line_clipboard = lines.then(|| text.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub(crate) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let (text, lines) = self.clipboard_text();
        self.line_clipboard = lines.then(|| text.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if !lines {
            self.edit_with(cx, |b, s, _| ops::insert_text(b, s, ""));
            return;
        }
        self.edit_with(cx, |b, s, _| {
            let ranges = line_ranges(b, s);
            let mut removed = 0;
            let cursors: Vec<Selection> = ranges
                .iter()
                .map(|r| {
                    let pos = r.start - removed;
                    removed += r.len();
                    Selection::cursor(pos)
                })
                .collect();
            let mut after = SelectionSet::from_vec(cursors, 0).unwrap_or_default();
            after.clamp(b.len_chars().saturating_sub(removed));
            let edits = ranges.into_iter().map(|r| (r, String::new())).collect();
            let applied = b.edit_with_selections(edits, UndoGroup::New, s, &after);
            *s = after;
            applied
        });
    }

    pub(crate) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let whole_lines = self.line_clipboard.as_deref() == Some(text.as_str())
            && self.selections.iter().all(Selection::is_empty);
        if !whole_lines {
            self.edit_with(cx, |b, s, _| ops::paste(b, s, &text));
            return;
        }
        // Lines copied without a selection go above the cursor's line.
        self.edit_with(cx, |b, s, _| {
            let len = text.chars().count();
            let mut sels: Vec<Selection> = Vec::new();
            let mut raw = Vec::new();
            let mut shift = 0;
            for sel in s.iter() {
                let at = b.line_start(b.char_to_line(sel.head));
                if raw
                    .last()
                    .is_some_and(|(r, _): &(Range<usize>, String)| r.start == at)
                {
                    sels.push(Selection::cursor(sel.head + shift));
                    continue;
                }
                raw.push((at..at, text.clone()));
                shift += len;
                sels.push(Selection::cursor(sel.head + shift));
            }
            let after = SelectionSet::from_vec(sels, s.primary_index()).unwrap_or_default();
            b.break_undo_group();
            let applied = b.edit_with_selections(raw, UndoGroup::New, s, &after);
            *s = after;
            applied
        });
    }

    pub(crate) fn save_action(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(err) = self.save(cx) {
            log::error!("failed to save {}: {err}", self.title());
        }
    }
}
