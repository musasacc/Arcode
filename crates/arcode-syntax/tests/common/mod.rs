//! Shared helpers for integration tests.
#![allow(dead_code)]

use arcode_syntax::{HighlightSpan, InputEditData, Syntax, language_by_id, point_for_byte};
use ropey::Rope;

/// Parses `src` as language `id` and returns the rope and syntax.
pub fn parse(id: &str, src: &str) -> (Rope, Syntax) {
    let lang = language_by_id(id).expect("known language");
    let text = Rope::from_str(src);
    let syntax = Syntax::new(lang, &text).expect("grammar");
    (text, syntax)
}

/// Highlights every line of `text`.
pub fn highlight_all(syntax: &Syntax, text: &Rope) -> Vec<Vec<HighlightSpan>> {
    let lines = syntax.highlight_lines(text, 0..text.len_lines());
    for (line, spans) in lines.iter().enumerate() {
        assert_well_formed(text, line, spans);
    }
    lines
}

/// The text of `line` without its line break.
pub fn line_text(text: &Rope, line: usize) -> String {
    let s = text.line(line).to_string();
    s.trim_end_matches(['\n', '\r']).to_string()
}

/// Asserts spans are sorted, non-empty, non-overlapping, inside the line and
/// on char boundaries.
pub fn assert_well_formed(text: &Rope, line: usize, spans: &[HighlightSpan]) {
    let content = line_text(text, line);
    let mut prev_end = 0;
    for span in spans {
        assert!(span.range.start < span.range.end, "empty span {span:?}");
        assert!(span.range.start >= prev_end, "overlap at {span:?}");
        assert!(span.range.end <= content.len(), "{span:?} past line end");
        assert!(content.is_char_boundary(span.range.start));
        assert!(content.is_char_boundary(span.range.end));
        prev_end = span.range.end;
    }
}

/// Spans of one line as `(text, name)` pairs for readable asserts.
pub fn pieces(text: &Rope, line: usize, spans: &[HighlightSpan]) -> Vec<(String, &'static str)> {
    let content = line_text(text, line);
    spans
        .iter()
        .map(|span| (content[span.range.clone()].to_string(), span.name))
        .collect()
}

/// The capture name covering the first occurrence of `needle` on `line`
/// (requires one span to cover the whole needle).
pub fn name_of(
    text: &Rope,
    lines: &[Vec<HighlightSpan>],
    line: usize,
    needle: &str,
) -> Option<&'static str> {
    let content = line_text(text, line);
    let start = content.find(needle)?;
    let end = start + needle.len();
    lines[line]
        .iter()
        .find(|span| span.range.start <= start && span.range.end >= end)
        .map(|span| span.name)
}

/// Replaces `range` (bytes) with `insert`, returning the matching edit.
pub fn apply_edit(text: &mut Rope, range: std::ops::Range<usize>, insert: &str) -> InputEditData {
    let start_position = point_for_byte(text, range.start);
    let old_end_position = point_for_byte(text, range.end);
    let start_char = text.byte_to_char(range.start);
    let end_char = text.byte_to_char(range.end);
    text.remove(start_char..end_char);
    text.insert(start_char, insert);
    let new_end_byte = range.start + insert.len();
    InputEditData {
        start_byte: range.start,
        old_end_byte: range.end,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position: point_for_byte(text, new_end_byte),
    }
}
