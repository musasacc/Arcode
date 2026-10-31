//! Small text helpers shared by movement, ops and search.

use ropey::Rope;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

/// Word characters are alphanumerics and `_`.
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Horizontal whitespace (not line breaks).
pub(crate) fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Char index of the grapheme boundary after `char_idx` (clamped).
pub(crate) fn next_grapheme_boundary(rope: &Rope, char_idx: usize) -> usize {
    let len = rope.len_chars();
    if char_idx >= len {
        return len;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    // Bounded loop: each iteration either returns or advances by a chunk.
    for _ in 0..=rope.len_bytes() {
        match cursor.next_boundary(chunk, chunk_start) {
            Ok(None) => return len,
            Ok(Some(b)) => return rope.byte_to_char(b),
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_start += chunk.len();
                let (c, s, _, _) = rope.chunk_at_byte(chunk_start);
                chunk = c;
                chunk_start = s;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx, ctx_start, _, _) = rope.chunk_at_byte(n.saturating_sub(1));
                cursor.provide_context(ctx, ctx_start);
            }
            Err(_) => break,
        }
    }
    char_idx + 1
}

/// Char index of the grapheme boundary before `char_idx` (clamped).
pub(crate) fn prev_grapheme_boundary(rope: &Rope, char_idx: usize) -> usize {
    let char_idx = char_idx.min(rope.len_chars());
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    for _ in 0..=rope.len_bytes() {
        match cursor.prev_boundary(chunk, chunk_start) {
            Ok(None) => return 0,
            Ok(Some(b)) => return rope.byte_to_char(b),
            Err(GraphemeIncomplete::PrevChunk) => {
                let (c, s, _, _) = rope.chunk_at_byte(chunk_start.saturating_sub(1));
                chunk = c;
                chunk_start = s;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx, ctx_start, _, _) = rope.chunk_at_byte(n.saturating_sub(1));
                cursor.provide_context(ctx, ctx_start);
            }
            Err(_) => break,
        }
    }
    char_idx - 1
}

/// Number of chars and bytes and the number of `\n` in `text`, plus the
/// char and byte length after the last `\n` (or of the whole text).
pub(crate) struct TextExtent {
    pub chars: usize,
    pub bytes: usize,
    pub newlines: usize,
    pub last_line_chars: usize,
    pub last_line_bytes: usize,
    pub last_line_utf16: usize,
}

impl TextExtent {
    pub(crate) fn of(text: &str) -> Self {
        let mut ext = TextExtent {
            chars: 0,
            bytes: text.len(),
            newlines: 0,
            last_line_chars: 0,
            last_line_bytes: 0,
            last_line_utf16: 0,
        };
        for c in text.chars() {
            ext.chars += 1;
            if c == '\n' {
                ext.newlines += 1;
                ext.last_line_chars = 0;
                ext.last_line_bytes = 0;
                ext.last_line_utf16 = 0;
            } else {
                ext.last_line_chars += 1;
                ext.last_line_bytes += c.len_utf8();
                ext.last_line_utf16 += c.len_utf16();
            }
        }
        ext
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grapheme_boundaries() {
        // e + combining acute, flag (two regional indicators), CJK.
        let rope = Rope::from_str("e\u{301}🇩🇪中\n");
        assert_eq!(next_grapheme_boundary(&rope, 0), 2);
        assert_eq!(next_grapheme_boundary(&rope, 2), 4);
        assert_eq!(next_grapheme_boundary(&rope, 4), 5);
        assert_eq!(next_grapheme_boundary(&rope, 5), 6);
        assert_eq!(next_grapheme_boundary(&rope, 6), 6);
        assert_eq!(prev_grapheme_boundary(&rope, 6), 5);
        assert_eq!(prev_grapheme_boundary(&rope, 5), 4);
        assert_eq!(prev_grapheme_boundary(&rope, 4), 2);
        assert_eq!(prev_grapheme_boundary(&rope, 2), 0);
        assert_eq!(prev_grapheme_boundary(&rope, 0), 0);
        assert_eq!(prev_grapheme_boundary(&rope, 100), 5);
    }

    #[test]
    fn grapheme_across_chunks() {
        // Long text forces several rope chunks; boundaries must still work.
        let text = "a\u{301}".repeat(5000);
        let rope = Rope::from_str(&text);
        let mut pos = 0;
        let mut count = 0;
        while pos < rope.len_chars() {
            pos = next_grapheme_boundary(&rope, pos);
            count += 1;
        }
        assert_eq!(count, 5000);
        while pos > 0 {
            pos = prev_grapheme_boundary(&rope, pos);
            count -= 1;
        }
        assert_eq!(count, 0);
    }

    #[test]
    fn extent() {
        let e = TextExtent::of("ab\n😀c");
        assert_eq!(e.chars, 5);
        assert_eq!(e.bytes, 8);
        assert_eq!(e.newlines, 1);
        assert_eq!(e.last_line_chars, 2);
        assert_eq!(e.last_line_bytes, 5);
        assert_eq!(e.last_line_utf16, 3);
        assert!(is_word_char('_') && is_word_char('é') && !is_word_char('-'));
        assert!(is_blank('\t') && !is_blank('\n'));
    }
}
