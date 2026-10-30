//! Line/column helpers for the fake editor. Columns count chars, like
//! arcode-buffer.

use crate::bindings::arcode::plugin::types::{Position, Range};

/// Number of lines, counting a trailing empty line like a rope does.
pub(crate) fn line_count(text: &str) -> u32 {
    u32::try_from(text.split('\n').count()).unwrap_or(u32::MAX)
}

/// Byte offset of `pos`, or `None` if it lies outside the text.
pub(crate) fn offset(text: &str, pos: Position) -> Option<usize> {
    let mut line_start = 0;
    for _ in 0..pos.line {
        line_start += text[line_start..].find('\n')? + 1;
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |i| line_start + i);
    let line = &text[line_start..line_end];
    let column = usize::try_from(pos.column).ok()?;
    if column == line.chars().count() {
        return Some(line_end);
    }
    line.char_indices().nth(column).map(|(i, _)| line_start + i)
}

/// Byte range of `range`, or `None` if invalid or reversed.
pub(crate) fn byte_range(text: &str, range: Range) -> Option<std::ops::Range<usize>> {
    let start = offset(text, range.start)?;
    let end = offset(text, range.end)?;
    (start <= end).then_some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, column: u32) -> Position {
        Position { line, column }
    }

    #[test]
    fn offsets_count_chars() {
        let t = "héllo\nwörld";
        assert_eq!(offset(t, pos(0, 2)), Some(3));
        assert_eq!(offset(t, pos(0, 5)), Some(6));
        assert_eq!(offset(t, pos(1, 0)), Some(7));
        assert_eq!(offset(t, pos(1, 5)), Some(t.len()));
        assert_eq!(offset(t, pos(1, 6)), None);
        assert_eq!(offset(t, pos(2, 0)), None);
        assert_eq!(line_count(t), 2);
        assert_eq!(line_count("a\n"), 2);
    }

    #[test]
    fn reversed_ranges_are_invalid() {
        assert_eq!(
            byte_range(
                "abc",
                Range {
                    start: pos(0, 2),
                    end: pos(0, 1)
                }
            ),
            None
        );
        assert_eq!(
            byte_range(
                "abc",
                Range {
                    start: pos(0, 1),
                    end: pos(0, 3)
                }
            ),
            Some(1..3)
        );
    }
}
