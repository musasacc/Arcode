//! Applying edits and the [`Edit`] record.

use std::ops::Range;

use crate::buffer::Buffer;
use crate::changes::{Change, normalize_changes};
use crate::coords::{Point, PositionEncoding, TsPoint};
use crate::history::{AppliedEdit, UndoGroup};
use crate::selection::SelectionSet;
use crate::text_util::TextExtent;

/// One applied change.
///
/// Records returned together must be applied **sequentially**: each one's
/// coordinates refer to the text after all previous records were applied.
/// The byte/point fields follow tree-sitter's `InputEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Replaced char range (in the text before this edit).
    pub range: Range<usize>,
    /// Inserted text (always `\n` line endings).
    pub new_text: String,
    /// Byte offset of the start.
    pub start_byte: usize,
    /// Byte offset of the end of the replaced text (before the edit).
    pub old_end_byte: usize,
    /// Byte offset of the end of the inserted text (after the edit).
    pub new_end_byte: usize,
    /// Start as (row, byte column).
    pub start_position: TsPoint,
    /// End of the replaced text as (row, byte column).
    pub old_end_position: TsPoint,
    /// End of the inserted text as (row, byte column).
    pub new_end_position: TsPoint,
    /// Start as (line, char column).
    pub start_point: Point,
    /// End of the replaced text as (line, char column).
    pub old_end_point: Point,
    /// End of the inserted text as (line, char column).
    pub new_end_point: Point,
    start_utf16: usize,
    old_end_utf16: usize,
}

/// An LSP range: `(line, character)` start and end positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LspRange {
    /// Start `(line, character)`.
    pub start: (u32, u32),
    /// End `(line, character)` (exclusive).
    pub end: (u32, u32),
}

fn u32c(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Edit {
    /// Char index of the end of the inserted text (after the edit).
    pub fn new_end_char(&self) -> usize {
        self.range.start + self.new_text.chars().count()
    }

    /// The replaced range as an LSP range in `encoding`, valid for the
    /// document state just before this edit. Together with
    /// [`Edit::new_text`] this is one `TextDocumentContentChangeEvent`.
    pub fn lsp_range(&self, encoding: PositionEncoding) -> LspRange {
        let (sc, ec) = match encoding {
            PositionEncoding::Utf8 => (self.start_position.column, self.old_end_position.column),
            PositionEncoding::Utf16 => (self.start_utf16, self.old_end_utf16),
            PositionEncoding::Utf32 => (self.start_point.column, self.old_end_point.column),
        };
        LspRange {
            start: (u32c(self.start_point.line), u32c(sc)),
            end: (u32c(self.old_end_point.line), u32c(ec)),
        }
    }
}

impl Buffer {
    /// Applies `edits` (char ranges in the current text, non-overlapping,
    /// any order) atomically as one new undo step.
    ///
    /// Returns the [`Edit`] records in application order. Ranges are
    /// clamped; overlapping ranges are merged; `\r\n` in inserted text is
    /// normalized. No-op edits produce no records and no undo step.
    pub fn edit(&mut self, edits: Vec<(Range<usize>, String)>) -> Vec<Edit> {
        self.edit_grouped(edits, UndoGroup::New)
    }

    /// Like [`Buffer::edit`] but with explicit undo grouping.
    pub fn edit_grouped(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        group: UndoGroup,
    ) -> Vec<Edit> {
        let changes = normalize_changes(edits, self.len_chars());
        self.apply_changes(&changes, group, None)
    }

    /// Like [`Buffer::edit_grouped`], also recording the selections before
    /// and after the change so undo/redo can restore them.
    pub fn edit_with_selections(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        group: UndoGroup,
        before: &SelectionSet,
        after: &SelectionSet,
    ) -> Vec<Edit> {
        let changes = normalize_changes(edits, self.len_chars());
        self.apply_changes(&changes, group, Some((before.clone(), after.clone())))
    }

    /// Applies normalized changes and records them in the history.
    pub(crate) fn apply_changes(
        &mut self,
        changes: &[Change],
        group: UndoGroup,
        selections: Option<(SelectionSet, SelectionSet)>,
    ) -> Vec<Edit> {
        if changes.is_empty() {
            return Vec::new();
        }
        // Apply back to front so each change's pre-edit coordinates hold.
        let ops: Vec<(usize, usize, &str)> = changes
            .iter()
            .rev()
            .map(|c| (c.range.start, c.range.len(), c.text.as_str()))
            .collect();
        let (edits, applied) = self.apply_sequential(&ops);
        self.history.record(applied, group, selections);
        self.version += 1;
        edits
    }

    /// Applies `(start, old_len_chars, new_text)` operations one after the
    /// other, each in the coordinates left by the previous one.
    pub(crate) fn apply_sequential(
        &mut self,
        ops: &[(usize, usize, &str)],
    ) -> (Vec<Edit>, Vec<AppliedEdit>) {
        let mut edits = Vec::with_capacity(ops.len());
        let mut applied = Vec::with_capacity(ops.len());
        for &(start, old_len, text) in ops {
            let start = self.clamp_char(start);
            let end = self.clamp_char(start + old_len);
            let (edit, old_text) = self.apply_one(start..end, text);
            applied.push(AppliedEdit::new(start, old_text, text.to_string()));
            edits.push(edit);
        }
        (edits, applied)
    }

    fn apply_one(&mut self, range: Range<usize>, text: &str) -> (Edit, String) {
        let start_byte = self.rope.char_to_byte(range.start);
        let old_end_byte = self.rope.char_to_byte(range.end);
        let start_position = self.char_to_ts_point(range.start);
        let old_end_position = self.char_to_ts_point(range.end);
        let start_point = self.char_to_point(range.start);
        let old_end_point = self.char_to_point(range.end);
        let start_line_char = range.start - start_point.column;
        let start_utf16 = self.units_between(start_line_char, range.start, PositionEncoding::Utf16);
        let end_line_char = range.end - old_end_point.column;
        let old_end_utf16 = self.units_between(end_line_char, range.end, PositionEncoding::Utf16);
        let old_text = self.rope.slice(range.clone()).to_string();

        self.rope.remove(range.clone());
        self.rope.insert(range.start, text);

        let ext = TextExtent::of(text);
        let (new_end_position, new_end_point) = if ext.newlines == 0 {
            (
                TsPoint::new(start_position.row, start_position.column + ext.bytes),
                Point::new(start_point.line, start_point.column + ext.chars),
            )
        } else {
            (
                TsPoint::new(start_position.row + ext.newlines, ext.last_line_bytes),
                Point::new(start_point.line + ext.newlines, ext.last_line_chars),
            )
        };
        let edit = Edit {
            range,
            new_text: text.to_string(),
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + ext.bytes,
            start_position,
            old_end_position,
            new_end_position,
            start_point,
            old_end_point,
            new_end_point,
            start_utf16,
            old_end_utf16,
        };
        (edit, old_text)
    }
}
