//! Coordinate types and conversions (chars, bytes, points, LSP, tree-sitter).

use crate::buffer::Buffer;

/// A zero-based position: `line` and `column` in **chars**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Point {
    /// Zero-based line.
    pub line: usize,
    /// Zero-based column in chars.
    pub column: usize,
}

impl Point {
    /// Creates a point.
    pub fn new(line: usize, column: usize) -> Self {
        Point { line, column }
    }
}

/// A tree-sitter style point: `row` and `column` in **bytes**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TsPoint {
    /// Zero-based row.
    pub row: usize,
    /// Zero-based column in UTF-8 bytes.
    pub column: usize,
}

impl TsPoint {
    /// Creates a point.
    pub fn new(row: usize, column: usize) -> Self {
        TsPoint { row, column }
    }
}

/// The unit LSP `character` offsets are counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PositionEncoding {
    /// UTF-8 code units (bytes).
    Utf8,
    /// UTF-16 code units (the LSP default).
    #[default]
    Utf16,
    /// Unicode scalar values (chars).
    Utf32,
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Buffer {
    /// Converts a char index (clamped) to a [`Point`].
    pub fn char_to_point(&self, char_idx: usize) -> Point {
        let char_idx = self.clamp_char(char_idx);
        let line = self.char_to_line(char_idx);
        Point::new(line, char_idx - self.line_start(line))
    }

    /// Converts a [`Point`] to a char index. Lines past the end clamp to the
    /// buffer end, columns past the line end clamp to the line end.
    pub fn point_to_char(&self, point: Point) -> usize {
        if point.line >= self.line_count() {
            return self.len_chars();
        }
        self.line_start(point.line) + point.column.min(self.line_len(point.line))
    }

    /// Converts a char index (clamped) to a byte offset.
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.rope.char_to_byte(self.clamp_char(char_idx))
    }

    /// Converts a byte offset (clamped) to a char index. A byte inside a
    /// multi-byte char rounds down to that char.
    pub fn byte_to_char(&self, byte_idx: usize) -> usize {
        self.rope.byte_to_char(byte_idx.min(self.len_bytes()))
    }

    /// Converts a char index (clamped) to a tree-sitter point (byte column).
    pub fn char_to_ts_point(&self, char_idx: usize) -> TsPoint {
        let char_idx = self.clamp_char(char_idx);
        let line = self.char_to_line(char_idx);
        let byte = self.rope.char_to_byte(char_idx);
        TsPoint::new(line, byte - self.rope.char_to_byte(self.line_start(line)))
    }

    /// Converts a tree-sitter point to a char index (clamped like
    /// [`Buffer::point_to_char`]; mid-char byte columns round down).
    pub fn ts_point_to_char(&self, point: TsPoint) -> usize {
        if point.row >= self.line_count() {
            return self.len_chars();
        }
        let line_start = self.rope.char_to_byte(self.line_start(point.row));
        let line_end = self.char_to_byte(self.line_end(point.row));
        self.rope
            .byte_to_char((line_start + point.column).min(line_end))
    }

    /// Converts a char index (clamped) to an LSP `(line, character)` pair.
    pub fn to_lsp_position(&self, char_idx: usize, encoding: PositionEncoding) -> (u32, u32) {
        let char_idx = self.clamp_char(char_idx);
        let line = self.char_to_line(char_idx);
        let line_start = self.line_start(line);
        let character = self.units_between(line_start, char_idx, encoding);
        (to_u32(line), to_u32(character))
    }

    /// Converts an LSP position to a char index, clamped: a `character` past
    /// the line end clamps to the line end, a `line` past the end clamps to
    /// the buffer end, and a UTF-16 offset inside a surrogate pair (or a
    /// UTF-8 offset inside a char) rounds down.
    pub fn from_lsp_position(
        &self,
        line: u32,
        character: u32,
        encoding: PositionEncoding,
    ) -> usize {
        let line = line as usize;
        let character = character as usize;
        if line >= self.line_count() {
            return self.len_chars();
        }
        let start = self.line_start(line);
        let end = self.line_end(line);
        match encoding {
            PositionEncoding::Utf32 => start + character.min(end - start),
            PositionEncoding::Utf8 => {
                let sb = self.rope.char_to_byte(start);
                let eb = self.rope.char_to_byte(end);
                self.rope.byte_to_char((sb + character).min(eb))
            }
            PositionEncoding::Utf16 => {
                let su = self.rope.char_to_utf16_cu(start);
                let eu = self.rope.char_to_utf16_cu(end);
                self.rope.utf16_cu_to_char((su + character).min(eu))
            }
        }
    }

    /// Number of `encoding` units between two char indices (`from <= to`).
    pub(crate) fn units_between(
        &self,
        from: usize,
        to: usize,
        encoding: PositionEncoding,
    ) -> usize {
        match encoding {
            PositionEncoding::Utf32 => to - from,
            PositionEncoding::Utf8 => self.rope.char_to_byte(to) - self.rope.char_to_byte(from),
            PositionEncoding::Utf16 => {
                self.rope.char_to_utf16_cu(to) - self.rope.char_to_utf16_cu(from)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PositionEncoding::*;

    #[test]
    fn char_point_round_trip() {
        let b = Buffer::new("ab\n😀中\n\nxyz");
        for i in 0..=b.len_chars() {
            assert_eq!(b.point_to_char(b.char_to_point(i)), i);
        }
        assert_eq!(b.char_to_point(4), Point::new(1, 1));
        assert_eq!(b.char_to_point(999), Point::new(3, 3));
        assert_eq!(b.point_to_char(Point::new(1, 99)), 5);
        assert_eq!(b.point_to_char(Point::new(9, 0)), b.len_chars());
        assert_eq!(b.point_to_char(Point::new(2, 5)), 6);
    }

    #[test]
    fn char_byte() {
        let b = Buffer::new("a😀é");
        assert_eq!(b.char_to_byte(0), 0);
        assert_eq!(b.char_to_byte(1), 1);
        assert_eq!(b.char_to_byte(2), 5);
        assert_eq!(b.char_to_byte(3), 7);
        assert_eq!(b.char_to_byte(99), 7);
        assert_eq!(b.byte_to_char(3), 1); // inside the emoji
        assert_eq!(b.byte_to_char(5), 2);
        assert_eq!(b.byte_to_char(6), 2); // inside é
        assert_eq!(b.byte_to_char(99), 3);
        assert_eq!(b.clamp_char(99), 3);
    }

    #[test]
    fn ts_points() {
        let b = Buffer::new("é\n😀x");
        assert_eq!(b.char_to_ts_point(1), TsPoint::new(0, 2));
        assert_eq!(b.char_to_ts_point(3), TsPoint::new(1, 4));
        assert_eq!(b.ts_point_to_char(TsPoint::new(1, 4)), 3);
        assert_eq!(b.ts_point_to_char(TsPoint::new(1, 2)), 2);
        assert_eq!(b.ts_point_to_char(TsPoint::new(1, 99)), 4);
        assert_eq!(b.ts_point_to_char(TsPoint::new(7, 0)), 4);
    }

    #[test]
    fn lsp_utf16_surrogates() {
        // "a😀b": 😀 is two UTF-16 units.
        let b = Buffer::new("x\na😀b\n");
        assert_eq!(b.to_lsp_position(2, Utf16), (1, 0));
        assert_eq!(b.to_lsp_position(3, Utf16), (1, 1));
        assert_eq!(b.to_lsp_position(4, Utf16), (1, 3));
        assert_eq!(b.to_lsp_position(5, Utf16), (1, 4));
        assert_eq!(b.from_lsp_position(1, 1, Utf16), 3);
        assert_eq!(b.from_lsp_position(1, 2, Utf16), 3); // mid-pair rounds down
        assert_eq!(b.from_lsp_position(1, 3, Utf16), 4);
        assert_eq!(b.from_lsp_position(1, 4, Utf16), 5);
        assert_eq!(b.from_lsp_position(1, 50, Utf16), 5); // clamps to line end
        assert_eq!(b.from_lsp_position(9, 0, Utf16), b.len_chars());
        assert_eq!(b.from_lsp_position(2, 0, Utf16), 6);
    }

    #[test]
    fn lsp_utf8_and_utf32() {
        let b = Buffer::new("中a😀");
        assert_eq!(b.to_lsp_position(1, Utf8), (0, 3));
        assert_eq!(b.to_lsp_position(3, Utf8), (0, 8));
        assert_eq!(b.to_lsp_position(3, Utf32), (0, 3));
        assert_eq!(b.to_lsp_position(99, Utf32), (0, 3));
        assert_eq!(b.from_lsp_position(0, 4, Utf8), 2);
        assert_eq!(b.from_lsp_position(0, 2, Utf8), 0); // mid-char rounds down
        assert_eq!(b.from_lsp_position(0, 6, Utf8), 2);
        assert_eq!(b.from_lsp_position(0, 2, Utf32), 2);
        assert_eq!(b.from_lsp_position(0, 9, Utf32), 3);
    }

    #[test]
    fn unicode_breaks_are_not_rows_for_ts_and_lsp() {
        // FF, VT, NEL, U+2028, U+2029: only `\n` starts a row.
        let b = Buffer::new("a\u{c}\u{b}\u{85}\u{2028}\u{2029}b\nc");
        let c = b.len_chars() - 1;
        assert_eq!(b.char_to_point(c), Point::new(1, 0));
        assert_eq!(b.char_to_ts_point(c), TsPoint::new(1, 0));
        assert_eq!(
            b.char_to_ts_point(c - 1),
            TsPoint::new(0, 1 + 1 + 1 + 2 + 3 + 3 + 1)
        );
        assert_eq!(b.ts_point_to_char(TsPoint::new(1, 0)), c);
        assert_eq!(b.ts_point_to_char(TsPoint::new(0, 99)), c - 1);
        for enc in [Utf8, Utf16, Utf32] {
            assert_eq!(b.to_lsp_position(c, enc), (1, 0));
            assert_eq!(b.from_lsp_position(1, 0, enc), c);
            assert_eq!(b.from_lsp_position(0, 99, enc), c - 1);
        }
        assert_eq!(b.to_lsp_position(6, Utf16), (0, 6));
        assert_eq!(b.to_lsp_position(6, Utf8), (0, 11));
    }

    #[test]
    fn lsp_round_trip_all_encodings() {
        let b = Buffer::new("e\u{301}x\n𝄞中\n\n😀😀");
        for enc in [Utf8, Utf16, Utf32] {
            for i in 0..=b.len_chars() {
                let (l, c) = b.to_lsp_position(i, enc);
                assert_eq!(b.from_lsp_position(l, c, enc), i, "{enc:?} {i}");
            }
        }
    }
}
