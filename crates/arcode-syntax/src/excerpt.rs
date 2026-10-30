//! Highlighting very large documents without keeping a syntax tree.
//!
//! A full tree-sitter tree costs tens of bytes per source byte, so a 9 MB
//! file would hold hundreds of megabytes just for highlighting. For such
//! files [`highlight_excerpt`] parses only a window of lines around the
//! viewport, starting at a nearby top-level line so the parser sees whole
//! items, highlights the requested lines and drops the tree again.

use std::ops::Range;

use ropey::Rope;

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
/// multi-line construct (block comment, raw string); otherwise the first
/// lines may be colored as if the construct were not there.
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
    let slice = text.slice(text.line_to_char(first)..text.line_to_char(last));
    let excerpt = Rope::from(slice);
    let Some(syntax) = Syntax::new(language, &excerpt) else {
        return out;
    };
    let spans = syntax.highlight_lines(&excerpt, lines.start - first..end - first);
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
    fn finds_top_level_start() {
        let text = Rope::from_str("fn a() {\n    x\n}\nfn b() {\n    y\n");
        assert_eq!(excerpt_start(&text, 4), 3);
        assert_eq!(excerpt_start(&text, 2), 0);
    }
}
