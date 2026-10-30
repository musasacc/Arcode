//! Highlighting very large documents without keeping a syntax tree.
//!
//! A full tree-sitter tree costs tens of bytes per source byte, so a 9 MB
//! file would hold hundreds of megabytes just for highlighting. For such
//! files [`highlight_excerpt`] parses only a window of lines around the
//! viewport, starting at a nearby top-level line so the parser sees whole
//! items, highlights the requested lines and drops the tree again.

use std::ops::Range;

use ropey::Rope;

use crate::comment_scan::enclosing_block_comment;
use crate::{HighlightSpan, Language, Syntax};

/// How far back [`highlight_excerpt`] looks for a top-level line.
pub const EXCERPT_CONTEXT_LINES: usize = 400;
/// Excerpts larger than this many bytes are not highlighted at all (e.g. a
/// minified file whose single line is megabytes long).
pub const EXCERPT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Highlights `lines` of `text` by parsing only an excerpt around them.
///
/// Returns one `Vec` per requested line, like [`Syntax::highlight_lines`].
/// Results match a full parse whenever the excerpt starts outside any
/// multi-line string. An excerpt that starts inside a block comment is
/// detected by scanning back for the nearest comment delimiter and parsed
/// from the comment's opener (or with a synthetic opener in front when
/// that is too far back). Inside a long multi-line string (raw string,
/// docstring, template literal) the first lines may still be colored as
/// if the string were not there.
pub fn highlight_excerpt(
    language: &'static Language,
    text: &Rope,
    lines: Range<usize>,
) -> Vec<Vec<HighlightSpan>> {
    let requested = lines.end.saturating_sub(lines.start);
    let mut out = vec![Vec::new(); requested];
    let end = lines.end.min(text.len_lines());
    if lines.start >= end || !language.has_grammar() {
        return out;
    }
    let mut first = excerpt_start(text, lines.start);
    // Parse on to the next top-level line so the last item is complete.
    let mut last = excerpt_end(text, end);
    let size = |a: usize, b: usize| text.line_to_byte(b) - text.line_to_byte(a);
    // Shrink the context if the excerpt would be too large.
    while (first < lines.start || last > end) && size(first, last) > EXCERPT_MAX_BYTES {
        first = (first + (lines.start - first).div_ceil(2)).min(lines.start);
        last = end + (last - end) / 2;
    }
    if size(first, last) > EXCERPT_MAX_BYTES {
        return out;
    }
    // An excerpt starting inside a block comment would be parsed as code:
    // start it at the comment's opener, or, if that is too far back, put a
    // synthetic opener line in front of it.
    let mut prefix = None;
    if let Some(delims) = language.block_comment {
        let at = text.line_to_byte(first);
        if let Some(opener) = enclosing_block_comment(text, at, delims, language.line_comment) {
            let opener_line = text.byte_to_line(opener);
            if size(opener_line, last) <= EXCERPT_MAX_BYTES {
                first = opener_line;
            } else {
                prefix = Some(delims.0);
            }
        }
    }
    let slice = text.slice(text.line_to_char(first)..text.line_to_char(last));
    let (excerpt, shift) = match prefix {
        Some(open) => {
            let mut rope = Rope::from_str(open);
            rope.insert(rope.len_chars(), "\n");
            rope.append(Rope::from(slice));
            (rope, 1)
        }
        None => (Rope::from(slice), 0),
    };
    let Some(syntax) = Syntax::new(language, &excerpt) else {
        return out;
    };
    let rel = lines.start - first + shift..end - first + shift;
    let spans = syntax.highlight_lines(&excerpt, rel);
    for (slot, line) in out.iter_mut().zip(spans) {
        *slot = line;
    }
    out
}

/// The closest line at or before `line` (within [`EXCERPT_CONTEXT_LINES`])
/// that looks like the start of a top-level item: it begins in column 0
/// with something other than a closing bracket or a comment continuation.
fn excerpt_start(text: &Rope, line: usize) -> usize {
    let floor = line.saturating_sub(EXCERPT_CONTEXT_LINES);
    (floor..=line)
        .rev()
        .find(|&l| is_top_level(text, l))
        .unwrap_or(floor)
}

/// The first top-level line at or after `line` (within
/// [`EXCERPT_CONTEXT_LINES`]), or the end of that window.
fn excerpt_end(text: &Rope, line: usize) -> usize {
    let ceil = (line + EXCERPT_CONTEXT_LINES).min(text.len_lines());
    (line..ceil)
        .find(|&l| is_top_level(text, l))
        .unwrap_or(ceil)
}

fn is_top_level(text: &Rope, line: usize) -> bool {
    match text.line(line).chars().next() {
        Some(c) => !c.is_whitespace() && !matches!(c, '}' | ')' | ']' | '*' | ',' | '.' | ';'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_by_id;

    fn doc() -> Rope {
        let mut s = String::new();
        for i in 0..300 {
            s.push_str(&format!(
                "/// Doc {i}\nfn f{i}(x: u32) -> u32 {{\n    let s = \"{i}\";\n    x + {i}\n}}\n\n"
            ));
        }
        Rope::from_str(&s)
    }

    #[test]
    fn matches_full_parse_in_the_middle() {
        let rust = language_by_id("rust").expect("rust");
        let text = doc();
        let full = Syntax::new(rust, &text).expect("parse");
        for range in [0..10, 700..760, 1000..1100, 1790..1800] {
            let expected = full.highlight_lines(&text, range.clone());
            let got = highlight_excerpt(rust, &text, range.clone());
            assert_eq!(got, expected, "lines {range:?}");
        }
    }

    #[test]
    fn past_end_and_plain_text() {
        let rust = language_by_id("rust").expect("rust");
        let text = doc();
        let n = text.len_lines();
        let got = highlight_excerpt(rust, &text, n - 2..n + 3);
        assert_eq!(got.len(), 5);
        let plain = crate::plain_text();
        assert!(
            highlight_excerpt(plain, &text, 0..3)
                .iter()
                .all(Vec::is_empty)
        );
    }

    #[test]
    fn viewport_inside_a_long_block_comment_is_a_comment() {
        let rust = language_by_id("rust").expect("rust");
        let mut s = String::from("/* Large header comment\n");
        for i in 0..600 {
            s.push_str(&format!("fn f{i}() {{ let s = \"{i}\"; }}\n"));
        }
        s.push_str("*/\nfn real() { let x = \"a\"; }\n");
        let text = Rope::from_str(&s);
        let full = Syntax::new(rust, &text).expect("parse");
        for range in [443..470, 590..605] {
            let expected = full.highlight_lines(&text, range.clone());
            let got = highlight_excerpt(rust, &text, range.clone());
            assert_eq!(got, expected, "lines {range:?}");
        }
    }

    #[test]
    fn far_away_comment_opener_is_synthesized() {
        let rust = language_by_id("rust").expect("rust");
        let filler = "x".repeat(4000);
        let mut s = String::from("/* Huge comment\n");
        for i in 0..700 {
            s.push_str(&format!("fn f{i}() {{ let s = \"{filler}\"; }}\n"));
        }
        s.push_str("*/\nfn real() { let x = \"a\"; }\n");
        let text = Rope::from_str(&s);
        assert!(text.len_bytes() > EXCERPT_MAX_BYTES);
        let full = Syntax::new(rust, &text).expect("parse");
        for range in [650..660, 695..703] {
            let expected = full.highlight_lines(&text, range.clone());
            let got = highlight_excerpt(rust, &text, range.clone());
            assert_eq!(got, expected, "lines {range:?}");
        }
    }

    #[test]
    fn finds_top_level_start() {
        let text = Rope::from_str("fn a() {\n    x\n}\nfn b() {\n    y\n");
        assert_eq!(excerpt_start(&text, 4), 3);
        assert_eq!(excerpt_start(&text, 2), 0);
    }
}
