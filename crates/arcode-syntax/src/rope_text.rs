//! Helpers to feed a [`Rope`] to tree-sitter without copying it.

use std::ops::Range;

use ropey::Rope;
use tree_sitter::Node;

/// Returns the bytes of `text` from `byte` to the end of the chunk holding it.
///
/// This is the parser's input callback: tree-sitter reads the rope chunk by
/// chunk. Returns an empty slice at (or past) the end of the text.
pub(crate) fn chunk_from(text: &Rope, byte: usize) -> &[u8] {
    if byte >= text.len_bytes() {
        return &[];
    }
    let (chunk, chunk_start, _, _) = text.chunk_at_byte(byte);
    chunk
        .as_bytes()
        .get(byte - chunk_start..)
        .unwrap_or_default()
}

/// Iterates the bytes of `range` as rope chunks (for query text predicates).
///
/// The range is clamped to the text and widened to char boundaries, so a
/// stale tree never causes a panic.
pub(crate) fn byte_chunks(text: &Rope, range: Range<usize>) -> ByteChunks<'_> {
    let len = text.len_bytes();
    let start = text.byte_to_char(range.start.min(len));
    let end = text.byte_to_char(range.end.min(len)).max(start);
    let as_bytes: fn(&str) -> &[u8] = str::as_bytes;
    text.slice(start..end).chunks().map(as_bytes)
}

/// Iterator over a rope range as byte slices.
pub(crate) type ByteChunks<'a> = std::iter::Map<ropey::iter::Chunks<'a>, fn(&'a str) -> &'a [u8]>;

/// A text provider for `QueryCursor` that reads node text from the rope.
pub(crate) fn node_text<'r>(text: &'r Rope) -> impl for<'n> FnMut(Node<'n>) -> ByteChunks<'r> + 'r {
    move |node| byte_chunks(text, node.byte_range())
}

/// Whether `c` ends a line for ropey (with its default `unicode_lines`).
fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

/// Byte range of line `line`, excluding its line break (`\n`, `\r\n`, ...).
///
/// Returns `None` if `line` is past the last line.
pub(crate) fn line_content_range(text: &Rope, line: usize) -> Option<Range<usize>> {
    if line >= text.len_lines() {
        return None;
    }
    let start = text.line_to_byte(line);
    let slice = text.line(line);
    let mut len = slice.len_bytes();
    let chars = slice.len_chars();
    if chars > 0 {
        let last = slice.char(chars - 1);
        if is_line_break(last) {
            len -= last.len_utf8();
            if last == '\n' && chars > 1 && slice.char(chars - 2) == '\r' {
                len -= 1;
            }
        }
    }
    Some(start..start + len)
}

/// The rope line (ropey's line breaks, as in the editor) holding `byte`.
/// Offsets past the end are clamped.
pub(crate) fn line_of_byte(text: &Rope, byte: usize) -> usize {
    text.byte_to_line(byte.min(text.len_bytes()))
}

/// Rope lines spanned by the byte `range`, end exclusive. A range ending
/// right after a line break ends on that break's line.
pub(crate) fn line_span(text: &Rope, range: Range<usize>) -> Range<usize> {
    let start = line_of_byte(text, range.start);
    let end = range.end.min(text.len_bytes());
    let last = if end > range.start {
        line_of_byte(text, end - 1).max(start)
    } else {
        start
    };
    start..last + 1
}

/// Converts a byte offset into a tree-sitter style `(row, byte column)`.
///
/// Like tree-sitter, only `'\n'` starts a new row: a lone `'\r'`, U+2028
/// and the other Unicode breaks that ropey treats as line ends do not
/// (CRLF counts once, at its `'\n'`). The column is the byte distance from
/// the last `'\n'`. Offsets past the end are clamped.
///
/// Cost is linear in `byte` (a vectorized newline count over the rope's
/// chunks), so callers that already track rows, such as the buffer's edit
/// conversion, should prefer their own positions.
pub fn point_for_byte(text: &Rope, byte: usize) -> (usize, usize) {
    let byte = byte.min(text.len_bytes());
    let mut row = 0;
    let mut row_start = 0;
    let mut chunk_start = 0;
    for chunk in text.chunks() {
        if chunk_start >= byte {
            break;
        }
        let bytes = chunk.as_bytes();
        let bytes = bytes.get(..byte - chunk_start).unwrap_or(bytes);
        row += memchr::memchr_iter(b'\n', bytes).count();
        if let Some(last) = memchr::memrchr(b'\n', bytes) {
            row_start = chunk_start + last + 1;
        }
        chunk_start += chunk.len();
    }
    (row, byte - row_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_from_reads_whole_text() {
        let text: String = (0..5000).map(|i| format!("line {i}\n")).collect();
        let rope = Rope::from_str(&text);
        let mut out = Vec::new();
        while out.len() < rope.len_bytes() {
            let chunk = chunk_from(&rope, out.len());
            assert!(!chunk.is_empty());
            out.extend_from_slice(chunk);
        }
        assert_eq!(out, text.as_bytes());
        assert!(chunk_from(&rope, rope.len_bytes()).is_empty());
        assert!(chunk_from(&rope, rope.len_bytes() + 10).is_empty());
    }

    #[test]
    fn byte_chunks_clamps_and_rounds() {
        let rope = Rope::from_str("aé b");
        let bytes: Vec<u8> = byte_chunks(&rope, 0..100).flatten().copied().collect();
        assert_eq!(bytes, "aé b".as_bytes());
        // Middle of 'é' widens to the whole char.
        let bytes: Vec<u8> = byte_chunks(&rope, 2..3).flatten().copied().collect();
        assert_eq!(bytes, "é".as_bytes());
    }

    #[test]
    fn line_ranges_exclude_breaks() {
        let rope = Rope::from_str("ab\r\ncd\nef\u{2028}g");
        assert_eq!(line_content_range(&rope, 0), Some(0..2));
        assert_eq!(line_content_range(&rope, 1), Some(4..6));
        assert_eq!(line_content_range(&rope, 2), Some(7..9));
        assert_eq!(line_content_range(&rope, 3), Some(12..13));
        assert_eq!(line_content_range(&rope, 4), None);
        let empty = Rope::new();
        assert_eq!(line_content_range(&empty, 0), Some(0..0));
    }

    #[test]
    fn line_spans_use_rope_lines() {
        let rope = Rope::from_str("a\u{2028}b\rc\nd\n");
        assert_eq!(line_of_byte(&rope, 0), 0);
        assert_eq!(line_of_byte(&rope, 4), 1);
        assert_eq!(line_of_byte(&rope, 6), 2);
        assert_eq!(line_of_byte(&rope, 99), 4);
        // Ends right after "c\n": still line 2.
        assert_eq!(line_span(&rope, 4..8), 1..3);
        assert_eq!(line_span(&rope, 8..9), 3..4);
        assert_eq!(line_span(&rope, 5..5), 1..2);
        assert_eq!(line_span(&rope, 0..999), 0..4);
    }

    #[test]
    fn points_use_byte_columns() {
        let rope = Rope::from_str("é\nxé");
        assert_eq!(point_for_byte(&rope, 0), (0, 0));
        assert_eq!(point_for_byte(&rope, 2), (0, 2));
        assert_eq!(point_for_byte(&rope, 3), (1, 0));
        assert_eq!(point_for_byte(&rope, 6), (1, 3));
        assert_eq!(point_for_byte(&rope, 99), (1, 3));
    }

    #[test]
    fn points_count_only_line_feeds() {
        let rope = Rope::from_str("a\r\nb\rc\u{2028}d\u{85}e\x0Bf\n");
        assert_eq!(point_for_byte(&rope, 2), (0, 2));
        assert_eq!(point_for_byte(&rope, 3), (1, 0));
        // 'c' after a lone CR stays on row 1.
        assert_eq!(point_for_byte(&rope, 5), (1, 2));
        let f = "a\r\nb\rc\u{2028}d\u{85}e\x0B".len();
        assert_eq!(point_for_byte(&rope, f), (1, f - 3));
        assert_eq!(point_for_byte(&rope, f + 2), (2, 0));
    }

    #[test]
    fn points_across_chunks() {
        let text: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let rope = Rope::from_str(&text);
        for row in [0, 1, 999, 12_345, 19_999] {
            let start = rope.line_to_byte(row);
            assert_eq!(point_for_byte(&rope, start), (row, 0));
            assert_eq!(point_for_byte(&rope, start + 3), (row, 3));
        }
        assert_eq!(point_for_byte(&rope, text.len()), (20_000, 0));
    }
}
