//! Word motions: `w W e E b B ge gE`.
//!
//! The motions step over whole grapheme clusters and classify a cluster
//! by its base char, so a combining mark never splits a word.

use arcode_buffer::{Buffer, movement};

use crate::text::{Class, ch, class, is_empty_line_at};

/// Start of the grapheme after the one at `i`.
fn nx(b: &Buffer, i: usize) -> usize {
    movement::right(b, i).max(i + 1)
}

/// Start of the grapheme before `i`.
fn pv(b: &Buffer, i: usize) -> usize {
    movement::left(b, i).min(i.saturating_sub(1))
}

/// Class of the grapheme starting at `i`.
fn cls(b: &Buffer, i: usize, big: bool) -> Class {
    class(ch(b, i), big)
}

/// Start of the next word (`w`). An empty line counts as a word. With
/// `stop_eol` (the last step of an operator's `w`), stops at the end of the
/// line instead of crossing to the next word.
pub(crate) fn next_word_start(b: &Buffer, pos: usize, big: bool, stop_eol: bool) -> usize {
    let n = b.len_chars();
    if pos >= n {
        return n;
    }
    let mut i = pos;
    let c0 = cls(b, i, big);
    if c0 != Class::Space {
        while i < n && cls(b, i, big) == c0 {
            i = nx(b, i);
        }
    }
    while i < n {
        let c = ch(b, i);
        if c == '\n' {
            if stop_eol && i > pos {
                return i;
            }
            i = nx(b, i);
            if stop_eol || i >= n || ch(b, i) == '\n' {
                return i.min(n);
            }
            continue;
        }
        if !c.is_whitespace() {
            break;
        }
        i = nx(b, i);
    }
    i.min(n)
}

/// End of the current or next word (`e`); always moves at least one
/// grapheme. Returns the start of the word's last grapheme.
pub(crate) fn next_word_end(b: &Buffer, pos: usize, big: bool) -> usize {
    let n = b.len_chars();
    if n == 0 {
        return 0;
    }
    let mut i = nx(b, pos);
    while i < n && cls(b, i, big) == Class::Space {
        i = nx(b, i);
    }
    if i >= n {
        return pv(b, n);
    }
    let c = cls(b, i, big);
    loop {
        let j = nx(b, i);
        if j < n && cls(b, j, big) == c {
            i = j;
        } else {
            return i;
        }
    }
}

/// Start of the current or previous word (`b`). Stops at empty lines.
pub(crate) fn prev_word_start(b: &Buffer, pos: usize, big: bool) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pv(b, pos);
    while cls(b, i, big) == Class::Space {
        if is_empty_line_at(b, i) || i == 0 {
            return i;
        }
        i = pv(b, i);
    }
    let c = cls(b, i, big);
    while i > 0 {
        let j = pv(b, i);
        if cls(b, j, big) != c {
            break;
        }
        i = j;
    }
    i
}

/// End of the previous word (`ge`). Stops at empty lines.
pub(crate) fn prev_word_end(b: &Buffer, pos: usize, big: bool) -> usize {
    let mut i = pos;
    let c0 = cls(b, pos, big);
    if c0 != Class::Space {
        while i > 0 {
            let j = pv(b, i);
            if cls(b, j, big) != c0 {
                break;
            }
            i = j;
        }
    }
    if i == 0 {
        return 0;
    }
    i = pv(b, i);
    loop {
        if cls(b, i, big) != Class::Space || is_empty_line_at(b, i) || i == 0 {
            return i;
        }
        i = pv(b, i);
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

    #[test]
    fn combining_marks_stay_in_their_word() {
        let b = Buffer::new("cafe\u{301} x\u{301}y");
        assert_eq!(next_word_start(&b, 0, false, false), 6);
        assert_eq!(next_word_end(&b, 0, false), 3);
        assert_eq!(next_word_end(&b, 3, false), 8);
        assert_eq!(prev_word_start(&b, 6, false), 0);
        assert_eq!(prev_word_end(&b, 6, false), 3);
        assert_eq!(prev_word_start(&b, 9, false), 6);
    }
}
