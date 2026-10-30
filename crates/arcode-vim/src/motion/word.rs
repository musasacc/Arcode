//! Word motions: `w W e E b B ge gE`.

use arcode_buffer::Buffer;

use crate::text::{Class, ch, class, is_empty_line_at};

/// Start of the next word (`w`). An empty line counts as a word. With
/// `stop_eol` (the last step of an operator's `w`), stops at the end of the
/// line instead of crossing to the next word.
pub(crate) fn next_word_start(b: &Buffer, pos: usize, big: bool, stop_eol: bool) -> usize {
    let n = b.len_chars();
    if pos >= n {
        return n;
    }
    let mut i = pos;
    let c0 = class(ch(b, i), big);
    if c0 != Class::Space {
        while i < n && class(ch(b, i), big) == c0 {
            i += 1;
        }
    }
    while i < n {
        let c = ch(b, i);
        if c == '\n' {
            if stop_eol && i > pos {
                return i;
            }
            i += 1;
            if stop_eol || i >= n || ch(b, i) == '\n' {
                return i;
            }
            continue;
        }
        if !c.is_whitespace() {
            break;
        }
        i += 1;
    }
    i
}

/// End of the current or next word (`e`); always moves at least one char.
pub(crate) fn next_word_end(b: &Buffer, pos: usize, big: bool) -> usize {
    let n = b.len_chars();
    if n == 0 {
        return 0;
    }
    let mut i = pos + 1;
    while i < n && class(ch(b, i), big) == Class::Space {
        i += 1;
    }
    if i >= n {
        return n - 1;
    }
    let c = class(ch(b, i), big);
    while i + 1 < n && class(ch(b, i + 1), big) == c {
        i += 1;
    }
    i
}

/// Start of the current or previous word (`b`). Stops at empty lines.
pub(crate) fn prev_word_start(b: &Buffer, pos: usize, big: bool) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    while class(ch(b, i), big) == Class::Space {
        if is_empty_line_at(b, i) || i == 0 {
            return i;
        }
        i -= 1;
    }
    let c = class(ch(b, i), big);
    while i > 0 && class(ch(b, i - 1), big) == c {
        i -= 1;
    }
    i
}

/// End of the previous word (`ge`). Stops at empty lines.
pub(crate) fn prev_word_end(b: &Buffer, pos: usize, big: bool) -> usize {
    let mut i = pos;
    let c0 = class(ch(b, pos), big);
    if c0 != Class::Space {
        while i > 0 && class(ch(b, i - 1), big) == c0 {
            i -= 1;
        }
    }
    if i == 0 {
        return 0;
    }
    i -= 1;
    loop {
        if class(ch(b, i), big) != Class::Space || is_empty_line_at(b, i) || i == 0 {
            return i;
        }
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        let b = Buffer::new("foo.bar  baz\n\nqux");
        assert_eq!(next_word_start(&b, 0, false, false), 3);
        assert_eq!(next_word_start(&b, 0, true, false), 9);
        assert_eq!(next_word_start(&b, 9, false, false), 13);
        assert_eq!(next_word_start(&b, 13, false, false), 14);
        assert_eq!(next_word_start(&b, 9, false, true), 12);
        assert_eq!(next_word_end(&b, 0, false), 2);
        assert_eq!(next_word_end(&b, 2, false), 3);
        assert_eq!(next_word_end(&b, 9, false), 11);
        assert_eq!(next_word_end(&b, 11, false), 16);
        assert_eq!(prev_word_start(&b, 14, false), 13);
        assert_eq!(prev_word_start(&b, 13, false), 9);
        assert_eq!(prev_word_start(&b, 9, true), 0);
        assert_eq!(prev_word_end(&b, 14, false), 13);
        assert_eq!(prev_word_end(&b, 13, false), 11);
        assert_eq!(prev_word_end(&b, 9, false), 6);
        assert_eq!(prev_word_end(&b, 4, false), 3);
    }
}
