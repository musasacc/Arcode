//! Finding the block comment that encloses a position by scanning back for
//! the nearest comment delimiter. Used by [`crate::highlight_excerpt`] so an
//! excerpt that starts inside a long block comment is parsed as a comment.

use memchr::memmem;
use ropey::Rope;

/// At most this many bytes are scanned backwards.
const SCAN_LIMIT: usize = 8 * 1024 * 1024;

/// Byte offset of the opener of the block comment that contains `byte`.
///
/// This is a textual heuristic: the nearest delimiter before `byte` must be
/// `open`, and the text before that opener on its line must not contain the
/// line comment token or an unterminated `"` string (then the opener is
/// probably not a real one and `None` is returned).
pub(crate) fn enclosing_block_comment(
    text: &Rope,
    byte: usize,
    (open, close): (&str, &str),
    line_comment: Option<&str>,
) -> Option<usize> {
    let (found, is_open) = last_delimiter(text, byte, open, close)?;
    if !is_open {
        return None;
    }
    let line_start = text.line_to_byte(text.byte_to_line(found));
    let before = text.byte_slice(line_start..found).to_string();
    let in_line_comment = line_comment.is_some_and(|t| before.contains(t));
    let in_string = before.matches('"').count() % 2 == 1;
    (!in_line_comment && !in_string).then_some(found)
}

/// The nearest occurrence of `open` or `close` that ends at or before
/// `end`: its byte offset and whether it is `open`.
fn last_delimiter(text: &Rope, end: usize, open: &str, close: &str) -> Option<(usize, bool)> {
    if open.is_empty() || close.is_empty() || end == 0 {
        return None;
    }
    let end = end.min(text.len_bytes());
    let overlap = open.len().max(close.len()) - 1;
    let (first, mut start, _, _) = text.chunk_at_byte(end);
    let mut chunk: &[u8] = &first.as_bytes()[..end - start];
    // The first `overlap` bytes after `chunk` (already scanned), so that a
    // delimiter crossing a chunk boundary is still found.
    let mut carry: Vec<u8> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut scanned = 0;
    loop {
        buf.clear();
        buf.extend_from_slice(chunk);
        buf.extend_from_slice(&carry);
        // Only matches that start inside `chunk` are new.
        let find = |d: &str| {
            let limit = (chunk.len() + d.len() - 1).min(buf.len());
            memmem::rfind(&buf[..limit], d.as_bytes())
        };
        let hit = match (find(open), find(close)) {
            (Some(o), Some(c)) => Some(if o >= c { (o, true) } else { (c, false) }),
            (Some(o), None) => Some((o, true)),
            (None, Some(c)) => Some((c, false)),
            (None, None) => None,
        };
        if let Some((i, is_open)) = hit {
            return Some((start + i, is_open));
        }
        scanned += chunk.len();
        if scanned > SCAN_LIMIT {
            return None;
        }
        carry = buf[..overlap.min(buf.len())].to_vec();
        if start == 0 {
            return None;
        }
        let (prev, prev_start, _, _) = text.chunk_at_byte(start - 1);
        start = prev_start;
        chunk = prev.as_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: (&str, &str) = ("/*", "*/");

    #[test]
    fn finds_the_opener_of_an_enclosing_comment() {
        let text = Rope::from_str("fn a() {}\n/* start\nfn b() {}\nlet x = 1;\n*/\nfn c() {}\n");
        let line = |l: usize| text.line_to_byte(l);
        assert_eq!(
            enclosing_block_comment(&text, line(3), C, Some("//")),
            Some(10)
        );
        assert_eq!(enclosing_block_comment(&text, line(1), C, Some("//")), None);
        assert_eq!(enclosing_block_comment(&text, line(5), C, Some("//")), None);
    }

    #[test]
    fn ignores_openers_in_line_comments_and_strings() {
        let text = Rope::from_str("// see /* here\nlet s = \"/*\";\nfn x() {}\n");
        let end = text.line_to_byte(2);
        assert_eq!(enclosing_block_comment(&text, end, C, Some("//")), None);
    }

    #[test]
    fn scans_across_chunk_boundaries() {
        // Ropey chunks are about 1 KB: put the opener far back so many
        // chunks (and likely a boundary through a delimiter) are crossed.
        for pad in 0..8 {
            let mut s = "x".repeat(1000 + pad);
            s.push_str("\n/*");
            let opener = s.len() - 2;
            for i in 0..2000 {
                s.push_str(&format!("line {i} with code-like fn text\n"));
            }
            let text = Rope::from_str(&s);
            let end = text.len_bytes();
            assert_eq!(enclosing_block_comment(&text, end, C, None), Some(opener));
        }
    }
}
