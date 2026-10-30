//! Viewport highlighting: capture spans for a range of lines.

use std::ops::Range;

use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::QueryCursor;

use crate::Syntax;
use crate::resolve::{Capture, resolve_captures};
use crate::rope_text::{line_content_range, node_text};

/// Every capture name the built-in highlight queries may produce.
///
/// Themes style these; lookup falls back along dotted prefixes
/// (`function.method` -> `function`). Queries must only use names from here.
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "function.macro",
    "function.method",
    "keyword",
    "keyword.control",
    "keyword.operator",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regex",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// A highlighted region within one line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HighlightSpan {
    /// Byte range within the line's text (line break excluded).
    pub range: Range<usize>,
    /// Capture name, e.g. `"keyword"` or `"function.method"`.
    pub name: &'static str,
}

impl Syntax {
    /// Highlights `lines` (zero-based, end exclusive) of `text`.
    ///
    /// Returns exactly one `Vec` per requested line (lines past the end of
    /// the text get an empty `Vec`). Spans in a line are sorted and never
    /// overlap; where captures nest the innermost node wins, and for the same
    /// node the later query pattern wins. Only the requested lines are
    /// queried, so the cost follows the viewport, not the file size.
    ///
    /// `text` must be the text this tree was last parsed from.
    pub fn highlight_lines(&self, text: &Rope, lines: Range<usize>) -> Vec<Vec<HighlightSpan>> {
        let requested = lines.end.saturating_sub(lines.start);
        let mut out = vec![Vec::new(); requested];
        let Some(query) = self.language().highlight_query() else {
            return out;
        };
        let last = lines.end.min(text.len_lines());
        if lines.start >= last {
            return out;
        }
        let window = text.line_to_byte(lines.start)..text.line_to_byte(last);
        let names = query.capture_names();

        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(window.clone());
        let mut captures = cursor.captures(query, self.tree().root_node(), node_text(text));
        let mut found = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let Some(capture) = matched.captures.get(*index) else {
                continue;
            };
            let Some(&name) = names.get(capture.index as usize) else {
                continue;
            };
            if name.starts_with('_') {
                continue;
            }
            found.push(Capture {
                range: capture.node.byte_range(),
                pattern: matched.pattern_index,
                name,
            });
        }
        let spans = resolve_captures(found, window);
        split_into_lines(text, lines.start..last, &spans, &mut out);
        out
    }
}

/// Splits absolute, sorted, non-overlapping spans into per-line spans.
fn split_into_lines(
    text: &Rope,
    lines: Range<usize>,
    spans: &[(Range<usize>, &'static str)],
    out: &mut [Vec<HighlightSpan>],
) {
    let mut first = 0;
    for (slot, line) in out.iter_mut().zip(lines) {
        let Some(content) = line_content_range(text, line) else {
            break;
        };
        while spans
            .get(first)
            .is_some_and(|(range, _)| range.end <= content.start)
        {
            first += 1;
        }
        for (range, name) in &spans[first..] {
            if range.start >= content.end {
                break;
            }
            let start = range.start.max(content.start);
            let end = range.end.min(content.end);
            if start < end {
                slot.push(HighlightSpan {
                    range: start - content.start..end - content.start,
                    name,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{all_languages, language_by_id};

    #[test]
    fn builtin_queries_compile_and_use_known_names() {
        for lang in all_languages().iter().filter(|l| l.has_grammar()) {
            if let Err(err) = lang.compile_highlights_query() {
                panic!("{err}");
            }
            let query = lang.highlight_query().expect("query");
            for name in query.capture_names() {
                assert!(
                    name.starts_with('_') || HIGHLIGHT_NAMES.contains(name),
                    "{}: capture @{name} missing from HIGHLIGHT_NAMES",
                    lang.name
                );
            }
        }
    }

    #[test]
    fn query_is_cached() {
        let rust = language_by_id("rust").expect("rust");
        let a = rust.highlight_query().expect("query") as *const _;
        let b = rust.highlight_query().expect("query") as *const _;
        assert_eq!(a, b);
    }

    #[test]
    fn names_are_unique() {
        for (i, a) in HIGHLIGHT_NAMES.iter().enumerate() {
            assert!(!HIGHLIGHT_NAMES[i + 1..].contains(a), "duplicate {a}");
        }
    }

    #[test]
    fn out_of_range_lines_are_empty() {
        let rust = language_by_id("rust").expect("rust");
        let text = Rope::from_str("fn a() {}\n");
        let syntax = Syntax::new(rust, &text).expect("parse");
        let lines = syntax.highlight_lines(&text, 5..8);
        assert_eq!(lines, vec![Vec::new(); 3]);
        #[allow(clippy::reversed_empty_ranges)]
        let lines = syntax.highlight_lines(&text, 3..1);
        assert!(lines.is_empty());
        let lines = syntax.highlight_lines(&text, 0..4);
        assert_eq!(lines.len(), 4);
        assert!(!lines[0].is_empty());
    }
}
