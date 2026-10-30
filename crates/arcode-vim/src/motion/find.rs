//! Char finds (`f t F T`), bracket matching (`%`) and paragraphs (`{ }`).

use arcode_buffer::{Buffer, movement};

use crate::text::{ch, last_line, line_empty, line_of};

/// A `f`/`t`/`F`/`T` search, remembered for `;` and `,`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FindSpec {
    pub ch: char,
    pub forward: bool,
    pub till: bool,
}

impl FindSpec {
    /// The same find in the other direction (`,`).
    pub(crate) fn reversed(self) -> Self {
        FindSpec {
            forward: !self.forward,
            ..self
        }
    }
}

/// The `count`-th occurrence of the find on the current line, stepping
/// over grapheme clusters (`t` stops before the target's cluster).
/// `repeat` (for `;`/`,`) skips a `t`/`T` target right next to the cursor.
pub(crate) fn find_char(
    b: &Buffer,
    pos: usize,
    spec: FindSpec,
    count: usize,
    repeat: bool,
) -> Option<usize> {
    let line = line_of(b, pos);
    let (start, end) = (b.line_start(line), b.line_end(line));
    let skip = spec.till && repeat;
    let mut found = 0;
    if spec.forward {
        let mut j = movement::right(b, pos);
        if skip {
            j = movement::right(b, j);
        }
        while j < end {
            if ch(b, j) == spec.ch {
                found += 1;
                if found == count {
                    return Some(if spec.till { movement::left(b, j) } else { j });
                }
            }
            j = movement::right(b, j).max(j + 1);
        }
    } else {
        if pos <= start || (skip && movement::left(b, pos) <= start) {
            return None;
        }
        let mut j = movement::left(b, pos);
        if skip {
            j = movement::left(b, j);
        }
        loop {
            if ch(b, j) == spec.ch {
                found += 1;
                if found == count {
                    return Some(if spec.till { movement::right(b, j) } else { j });
                }
            }
            if j <= start {
                break;
            }
            j = movement::left(b, j);
        }
    }
    None
}

const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// `%`: the bracket matching the first bracket at or after `pos` on its
/// line.
pub(crate) fn match_pair(b: &Buffer, pos: usize) -> Option<usize> {
    let end = b.line_end(line_of(b, pos));
    let start = (pos..end).find(|&i| PAIRS.iter().any(|&(o, c)| ch(b, i) == o || ch(b, i) == c))?;
    let c = ch(b, start);
    if let Some(&(open, close)) = PAIRS.iter().find(|p| p.0 == c) {
        find_close(b, start + 1, open, close)
    } else {
        let &(open, close) = PAIRS.iter().find(|p| p.1 == c)?;
        find_open(b, start, open, close)
    }
}

/// The unmatched `close` at or after `from`.
pub(crate) fn find_close(b: &Buffer, from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in b.text().chars_at(b.clamp_char(from)).enumerate() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(from + i);
            }
            depth -= 1;
        }
    }
    None
}

/// The unmatched `open` before `before`.
pub(crate) fn find_open(b: &Buffer, before: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut chars = b.text().chars_at(b.clamp_char(before));
    let mut i = b.clamp_char(before);
    while let Some(c) = chars.prev() {
        i -= 1;
        if c == close {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// `}`: start of the `count`-th empty line after the current paragraph,
/// or the end of the last line.
pub(crate) fn paragraph_forward(b: &Buffer, pos: usize, count: usize) -> usize {
    let last = last_line(b);
    let mut line = line_of(b, pos);
    for _ in 0..count {
        while line <= last && line_empty(b, line) {
            line += 1;
        }
        while line <= last && !line_empty(b, line) {
            line += 1;
        }
        if line > last {
            return b.line_end(last);
        }
    }
    b.line_start(line)
}

/// `{`: start of the `count`-th empty line before the current paragraph,
/// or the buffer start.
pub(crate) fn paragraph_backward(b: &Buffer, pos: usize, count: usize) -> usize {
    let mut line = line_of(b, pos);
    for _ in 0..count {
        while line > 0 && line_empty(b, line) {
            line -= 1;
        }
        while line > 0 && !line_empty(b, line) {
            line -= 1;
        }
    }
    b.line_start(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds() {
        let b = Buffer::new("a,b,c,d\nx,");
        let f = FindSpec {
            ch: ',',
            forward: true,
            till: false,
        };
        assert_eq!(find_char(&b, 0, f, 1, false), Some(1));
        assert_eq!(find_char(&b, 0, f, 3, false), Some(5));
        assert_eq!(find_char(&b, 0, f, 4, false), None);
        let t = FindSpec { till: true, ..f };
        assert_eq!(find_char(&b, 0, t, 1, false), Some(0));
        assert_eq!(find_char(&b, 0, t, 1, true), Some(2));
        assert_eq!(find_char(&b, 6, f.reversed(), 1, false), Some(5));
        assert_eq!(find_char(&b, 6, t.reversed(), 1, true), Some(4));
        assert_eq!(find_char(&b, 0, f.reversed(), 1, false), None);
    }

    #[test]
    fn pairs_and_paragraphs() {
        let b = Buffer::new("f(a[1], (b))\n\n\nx\ny\n");
        assert_eq!(match_pair(&b, 0), Some(11));
        assert_eq!(match_pair(&b, 11), Some(1));
        assert_eq!(match_pair(&b, 3), Some(5));
        assert_eq!(paragraph_forward(&b, 0, 1), 13);
        // The trailing `\n` ends line `y`; it does not start a paragraph.
        assert_eq!(paragraph_forward(&b, 0, 2), 18);
        assert_eq!(paragraph_forward(&b, 0, 3), 18);
        assert_eq!(paragraph_backward(&b, 17, 1), 14);
        assert_eq!(paragraph_backward(&b, 17, 2), 0);
        let b = Buffer::new("a\n\nb");
        assert_eq!(paragraph_forward(&b, 3, 1), b.len_chars());
    }
}
