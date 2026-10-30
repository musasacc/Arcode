//! Char and line helpers in Vim terms.
//!
//! Positions are char indices. A Normal-mode cursor at `pos` sits **on**
//! the char `pos` (the block covers `[pos, pos + 1)`).

use std::ops::Range;

use arcode_buffer::{Buffer, movement};

/// The char at `i`; past the end reads as `'\n'`.
pub(crate) fn ch(b: &Buffer, i: usize) -> char {
    b.char_at(i).unwrap_or('\n')
}

/// Line of `pos`.
pub(crate) fn line_of(b: &Buffer, pos: usize) -> usize {
    b.char_to_line(pos)
}

/// Index of the last Vim line. A trailing line break ends the last line
/// rather than starting an empty one, so the empty buffer line after it
/// does not count (as in Vim, which treats `\n` as a line terminator).
pub(crate) fn last_line(b: &Buffer) -> usize {
    let n = b.line_count().saturating_sub(1);
    if n > 0 && b.line_len(n) == 0 {
        n - 1
    } else {
        n
    }
}

/// True if a line break follows `line` (always the case before the
/// last buffer line, including the empty one after a trailing `\n`).
pub(crate) fn has_line_break(b: &Buffer, line: usize) -> bool {
    line + 1 < b.line_count()
}

/// Char column of `pos` within its line.
pub(crate) fn col_of(b: &Buffer, pos: usize) -> usize {
    let pos = b.clamp_char(pos);
    pos - b.line_start(b.char_to_line(pos))
}

/// Position at char column `col` of `line`, clamped to the line end.
pub(crate) fn pos_at_col(b: &Buffer, line: usize, col: usize) -> usize {
    let line = line.min(last_line(b));
    (b.line_start(line) + col).min(b.line_end(line))
}

/// The last char a Normal-mode cursor may sit on in `line` (the line start
/// for an empty line).
pub(crate) fn last_char(b: &Buffer, line: usize) -> usize {
    let start = b.line_start(line);
    let end = b.line_end(line);
    if end > start {
        movement::left(b, end).max(start)
    } else {
        start
    }
}

/// Clamps `pos` to a valid Normal-mode cursor position.
pub(crate) fn clamp_normal(b: &Buffer, pos: usize) -> usize {
    let pos = b.clamp_char(pos);
    let line = b.char_to_line(pos).min(last_line(b));
    pos.min(last_char(b, line))
}

/// First non-blank char of `line` (clamped to a Normal-mode position).
pub(crate) fn first_non_blank(b: &Buffer, line: usize) -> usize {
    let line = line.min(last_line(b));
    clamp_normal(b, movement::first_non_whitespace(b, line))
}

/// Space or tab.
pub(crate) fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Keyword char: alphanumeric or `_`.
pub(crate) fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Char class for word motions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Space,
    Word,
    Punct,
}

/// Class of `c`; with `big` (WORD) every non-blank is `Word`.
pub(crate) fn class(c: char, big: bool) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if big || is_word(c) {
        Class::Word
    } else {
        Class::Punct
    }
}

/// True if `pos` is the start of an empty line.
pub(crate) fn is_empty_line_at(b: &Buffer, pos: usize) -> bool {
    ch(b, pos) == '\n' && (pos == 0 || ch(b, pos - 1) == '\n')
}

/// True if `line` has no chars.
pub(crate) fn line_empty(b: &Buffer, line: usize) -> bool {
    b.line_len(line) == 0
}

/// True if `line` has only blanks.
pub(crate) fn line_blank(b: &Buffer, line: usize) -> bool {
    b.line_text(line).chars().all(is_blank)
}

/// Leading blanks of `line`.
pub(crate) fn indent_of(b: &Buffer, line: usize) -> String {
    b.line_text(line)
        .chars()
        .take_while(|&c| is_blank(c))
        .collect()
}

/// The range to delete for lines `first..=last`: whole lines with their
/// line breaks; for a block ending at a last line without a line break,
/// the preceding line break instead.
pub(crate) fn line_delete_range(b: &Buffer, first: usize, last: usize) -> Range<usize> {
    let last = last.min(last_line(b));
    if has_line_break(b, last) {
        b.line_start(first)..b.line_start(last + 1)
    } else if first > 0 {
        b.line_end(first - 1)..b.len_chars()
    } else {
        0..b.len_chars()
    }
}

/// Text of lines `first..=last`, each ending with `\n` (linewise register).
pub(crate) fn lines_text(b: &Buffer, first: usize, last: usize) -> String {
    let last = last.min(last_line(b));
    let mut s = b.slice_to_string(b.line_start(first)..b.line_end(last));
    s.push('\n');
    s
}

/// Display column of `pos`.
pub(crate) fn display_col(b: &Buffer, pos: usize, tab: usize) -> usize {
    movement::display_column(b, pos, tab)
}

/// Char on `line` at display column `col` and whether the line reaches
/// that column (`None` for a short line).
pub(crate) fn char_at_display_col(
    b: &Buffer,
    line: usize,
    col: usize,
    tab: usize,
) -> Option<usize> {
    let pos = movement::char_for_display_column(b, line, col, tab);
    (pos < b.line_end(line)).then_some(pos)
}

/// Lines `first..=last` touched by `range` (a range ending at a line start
/// does not include that line unless it is empty).
pub(crate) fn lines_of_range(b: &Buffer, range: &Range<usize>) -> (usize, usize) {
    let first = line_of(b, range.start);
    let mut last = line_of(b, range.end);
    if last > first && b.line_start(last) == range.end {
        last -= 1;
    }
    (first, last)
}

/// Text with the case of every char changed by `f`.
pub(crate) fn map_case(text: &str, f: fn(char) -> String) -> String {
    text.chars().map(f).collect()
}

/// Uppercase of `c` (may be several chars).
pub(crate) fn upper(c: char) -> String {
    c.to_uppercase().collect()
}

/// Lowercase of `c`.
pub(crate) fn lower(c: char) -> String {
    c.to_lowercase().collect()
}

/// `c` with its case toggled.
pub(crate) fn toggle(c: char) -> String {
    if c.is_uppercase() {
        lower(c)
    } else if c.is_lowercase() {
        upper(c)
    } else {
        c.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let b = Buffer::new("ab\n\n  cd");
        assert_eq!(last_char(&b, 0), 1);
        assert_eq!(last_char(&b, 1), 3);
        assert_eq!(clamp_normal(&b, 2), 1);
        assert_eq!(first_non_blank(&b, 2), 6);
        assert!(is_empty_line_at(&b, 3));
        assert!(!is_empty_line_at(&b, 2));
        assert_eq!(line_delete_range(&b, 0, 0), 0..3);
        assert_eq!(line_delete_range(&b, 2, 2), 3..8);
        assert_eq!(lines_text(&b, 1, 2), "\n  cd\n");
        assert_eq!(indent_of(&b, 2), "  ");
        assert_eq!(lines_of_range(&b, &(0..3)), (0, 0));
        assert_eq!(map_case("aB", toggle), "Ab");
        assert_eq!(char_at_display_col(&b, 0, 5, 4), None);
    }

    #[test]
    fn trailing_line_break_ends_the_last_line() {
        let b = Buffer::new("a\nb\n");
        assert_eq!(last_line(&b), 1);
        assert_eq!(clamp_normal(&b, 4), 2);
        assert_eq!(line_delete_range(&b, 1, 1), 2..4);
        assert_eq!(line_delete_range(&b, 0, 1), 0..4);
        assert_eq!(lines_text(&b, 1, 1), "b\n");
        let b = Buffer::new("\n");
        assert_eq!(last_line(&b), 0);
        assert_eq!(line_delete_range(&b, 0, 0), 0..1);
        let b = Buffer::new("a\n\n");
        assert_eq!(last_line(&b), 1);
        assert_eq!(clamp_normal(&b, 3), 2);
    }
}
