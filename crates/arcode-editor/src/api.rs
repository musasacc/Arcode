//! Programmatic editing and query APIs: scrolling, multi-range edits as one
//! undo step, text and word lookup, and coordinate conversions.

use std::ops::Range;

use arcode_buffer::{Point, PositionEncoding, Selection, SelectionSet, UndoGroup, ops};
use gpui::Context;

use crate::Editor;
use crate::editor::Autoscroll;

/// Where [`Editor::scroll_to_line`] puts the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAnchor {
    /// At the top of the viewport.
    Top,
    /// Vertically centered.
    Center,
    /// Scroll only as far as needed to show it.
    Fit,
}

/// Maps char offset `pos` through `edits` (sorted, non-overlapping char
/// ranges with their new lengths). Positions inside a replaced range move
/// to the end of the replacement.
pub(crate) fn map_offset(pos: usize, edits: &[(Range<usize>, usize)]) -> usize {
    let mut delta = 0isize;
    for (range, new_len) in edits {
        if range.end <= pos && !(range.is_empty() && range.start == pos) {
            delta += *new_len as isize - range.len() as isize;
        } else if range.start < pos {
            return (range.start as isize + delta) as usize + new_len;
        } else {
            break;
        }
    }
    (pos as isize + delta).max(0) as usize
}

impl Editor {
    /// Number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Scrolls so `line` is visible, placed by `anchor`. Takes effect on
    /// the next frame; the cursor does not move.
    pub fn scroll_to_line(&mut self, line: usize, anchor: ScrollAnchor, cx: &mut Context<Self>) {
        let line = line.min(self.buffer.line_count().saturating_sub(1));
        self.autoscroll = Some(Autoscroll::Line(line, anchor));
        cx.notify();
    }

    /// Char offset of `point` (clamped).
    pub fn point_to_offset(&self, point: Point) -> usize {
        let line = point.line.min(self.buffer.line_count().saturating_sub(1));
        let column = point.column.min(self.buffer.line_len(line));
        self.buffer.point_to_char(Point::new(line, column))
    }

    /// Position of char offset `offset` (clamped).
    pub fn offset_to_point(&self, offset: usize) -> Point {
        self.buffer.char_to_point(self.buffer.clamp_char(offset))
    }

    /// The text in `range` (clamped).
    pub fn text_in_range(&self, range: Range<Point>) -> String {
        let a = self.point_to_offset(range.start);
        let b = self.point_to_offset(range.end);
        self.buffer.slice_to_string(a.min(b)..a.max(b))
    }

    /// The document outline (functions, types, ...) from the syntax tree;
    /// empty for languages without an outline query and for large files.
    pub fn outline(&self) -> Vec<arcode_syntax::OutlineItem> {
        self.syntax
            .as_ref()
            .map(|s| s.symbols(self.buffer.text()))
            .unwrap_or_default()
    }

    /// The word at or just before `point` and its range, if any.
    pub fn word_at(&self, point: Point) -> Option<(Range<Point>, String)> {
        let pos = self.point_to_offset(point);
        let range = ops::word_range_at(&self.buffer, pos)
            .or_else(|| ops::word_range_at(&self.buffer, pos.checked_sub(1)?))?;
        let text = self.buffer.slice_to_string(range.clone());
        let points = self.offset_to_point(range.start)..self.offset_to_point(range.end);
        Some((points, text))
    }

    /// Converts `point` to an LSP `(line, character)` in `encoding`.
    pub fn point_to_lsp(&self, point: Point, encoding: PositionEncoding) -> (u32, u32) {
        self.buffer
            .to_lsp_position(self.point_to_offset(point), encoding)
    }

    /// Converts an LSP position in `encoding` to a buffer point (clamped).
    pub fn lsp_to_point(&self, line: u32, character: u32, encoding: PositionEncoding) -> Point {
        let pos = self.buffer.from_lsp_position(line, character, encoding);
        self.buffer.char_to_point(pos)
    }

    /// Replaces each range with its text as **one undo step** (e.g. an LSP
    /// workspace edit, a formatter result or a conflict resolution).
    /// Ranges must not overlap. Cursors move with the text; a cursor inside
    /// a replaced range lands at the end of its replacement. Returns
    /// `false` (and changes nothing) when read-only or nothing changed.
    pub fn apply_edits(
        &mut self,
        edits: Vec<(Range<Point>, String)>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.read_only || edits.is_empty() {
            return false;
        }
        let mut raw: Vec<(Range<usize>, String)> = edits
            .into_iter()
            .map(|(r, text)| {
                let a = self.point_to_offset(r.start);
                let b = self.point_to_offset(r.end);
                (a.min(b)..a.max(b), text.replace("\r\n", "\n"))
            })
            .collect();
        raw.sort_by_key(|(r, _)| (r.start, r.end));
        let shifts: Vec<(Range<usize>, usize)> = raw
            .iter()
            .map(|(r, t)| (r.clone(), t.chars().count()))
            .collect();
        let before = self.selections.clone();
        let mut after = before.clone();
        after.transform(|s| {
            Selection::new(map_offset(s.anchor, &shifts), map_offset(s.head, &shifts))
        });
        self.buffer.break_undo_group();
        let applied = self
            .buffer
            .edit_with_selections(raw, UndoGroup::New, &before, &after);
        self.buffer.break_undo_group();
        if applied.is_empty() {
            return false;
        }
        after.clamp(self.buffer.len_chars());
        self.selections = after;
        self.after_edits(applied, cx);
        true
    }

    /// Replaces the selections with cursors/ranges given as points.
    pub fn select_ranges(&mut self, ranges: Vec<Range<Point>>, cx: &mut Context<Self>) {
        let sels: Vec<Selection> = ranges
            .into_iter()
            .map(|r| Selection::new(self.point_to_offset(r.start), self.point_to_offset(r.end)))
            .collect();
        if let Some(set) = SelectionSet::from_vec(sels, 0) {
            self.set_selections(set, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::map_offset;

    #[test]
    fn offsets_shift_through_edits() {
        // "abcdef": replace 1..3 with 4 chars, insert 2 chars at 5.
        let edits = vec![(1..3, 4), (5..5, 2)];
        assert_eq!(map_offset(0, &edits), 0);
        assert_eq!(map_offset(1, &edits), 1);
        assert_eq!(map_offset(2, &edits), 5);
        assert_eq!(map_offset(3, &edits), 5);
        assert_eq!(map_offset(4, &edits), 6);
        assert_eq!(map_offset(5, &edits), 7);
        assert_eq!(map_offset(6, &edits), 10);
    }
}
