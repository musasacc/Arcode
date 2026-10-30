//! Platform text input (typing, IME composition, dead keys).
//!
//! The platform speaks UTF-16 offsets into the whole document; the buffer
//! uses char indices. Conversions go through ropey in O(log n).

use std::ops::Range;

use arcode_buffer::{Selection, SelectionSet, UndoGroup, ops};
use gpui::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, size,
};

use crate::Editor;

impl Editor {
    fn char_to_utf16(&self, char_idx: usize) -> usize {
        let rope = self.buffer.text();
        rope.char_to_utf16_cu(char_idx.min(rope.len_chars()))
    }

    fn char_from_utf16(&self, utf16: usize) -> usize {
        let rope = self.buffer.text();
        rope.utf16_cu_to_char(utf16.min(rope.len_utf16_cu()))
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.char_to_utf16(r.start)..self.char_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        let a = self.char_from_utf16(r.start);
        let b = self.char_from_utf16(r.end);
        a.min(b)..a.max(b)
    }

    /// Ranges the platform wants replaced: the marked text, the given range
    /// mirrored at every cursor, or the selections.
    fn target_ranges(&self, range_utf16: Option<Range<usize>>) -> Vec<Range<usize>> {
        if let Some(marked) = &self.marked {
            return marked.clone();
        }
        let Some(r) = range_utf16 else {
            return self.selections.iter().map(Selection::range).collect();
        };
        let r = self.range_from_utf16(&r);
        let head = self.selections.primary().head as isize;
        let (ds, de) = (r.start as isize - head, r.end as isize - head);
        let len = self.buffer.len_chars() as isize;
        self.selections
            .iter()
            .map(|s| {
                let h = s.head as isize;
                ((h + ds).clamp(0, len) as usize)..((h + de).clamp(0, len) as usize)
            })
            .collect()
    }

    /// Replaces every range with `text`; returns the inserted ranges and
    /// leaves cursors at their ends.
    fn replace_ranges(
        &mut self,
        mut ranges: Vec<Range<usize>>,
        text: &str,
        group: UndoGroup,
        cx: &mut Context<Self>,
    ) -> Vec<Range<usize>> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let len = text.chars().count();
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for r in ranges {
            match merged.last_mut() {
                Some(last) if r.start < last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        let mut inserted = Vec::with_capacity(merged.len());
        let mut delta = 0isize;
        for r in &merged {
            let start = (r.start as isize + delta).max(0) as usize;
            inserted.push(start..start + len);
            delta += len as isize - r.len() as isize;
        }
        let cursors = inserted.iter().map(|r| Selection::cursor(r.end)).collect();
        let primary = self
            .selections
            .primary_index()
            .min(inserted.len().saturating_sub(1));
        let after = SelectionSet::from_vec(cursors, primary).unwrap_or_default();
        let raw = merged.into_iter().map(|r| (r, text.clone())).collect();
        let edits = self
            .buffer
            .edit_with_selections(raw, group, &self.selections, &after);
        self.selections = after;
        self.after_edits(edits, cx);
        inserted
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.buffer.slice_to_string(range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.selections.primary();
        Some(UTF16Selection {
            range: self.range_to_utf16(&sel.range()),
            reversed: sel.is_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.marked.as_ref()?;
        let idx = self
            .selections
            .primary_index()
            .min(marked.len().saturating_sub(1));
        marked.get(idx).map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.marked.is_none() && range_utf16.is_none() {
            self.edit_with(cx, |b, s, cfg| ops::type_text(b, s, text, cfg));
            return;
        }
        let ranges = self.target_ranges(range_utf16);
        self.marked = None;
        self.replace_ranges(ranges, text, UndoGroup::Merge, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let ranges = self.target_ranges(range_utf16);
        let inserted = self.replace_ranges(ranges, new_text, UndoGroup::Merge, cx);
        if new_text.is_empty() {
            self.marked = None;
            return;
        }
        if let Some(sel) = new_selected_range {
            // Offsets are UTF-16 units within `new_text`.
            let to_chars = |u: usize| {
                let mut units = 0;
                new_text
                    .chars()
                    .take_while(|c| {
                        units += c.len_utf16();
                        units <= u
                    })
                    .count()
            };
            let (a, b) = (to_chars(sel.start), to_chars(sel.end));
            let sels = inserted
                .iter()
                .map(|r| Selection::new(r.start + a, r.start + b))
                .collect();
            if let Some(set) = SelectionSet::from_vec(sels, self.selections.primary_index()) {
                self.selections = set;
            }
        }
        self.marked = Some(inserted);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.clone()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = self.buffer.char_to_point(range.start);
        let end = self.buffer.char_to_point(range.end);
        let left = layout.text_left() + layout.x_for(start.line, start.column);
        let right = if end.line == start.line {
            layout.text_left() + layout.x_for(end.line, end.column)
        } else {
            left
        };
        let top = layout.line_top(start.line);
        Some(Bounds::new(
            point(left, top),
            size((right - left).max(Pixels::ZERO), layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.clone()?;
        let pos = self.char_for_position(&layout, point);
        Some(self.char_to_utf16(pos))
    }
}
