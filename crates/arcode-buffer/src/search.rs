//! Plain-text search over a buffer, streaming over the rope (no full-text
//! copies). Results are char ranges.
//!
//! Case-insensitive matching folds each char with its simple lowercase
//! mapping (chars whose lowercase form is several chars are compared as is).

use std::ops::Range;

use crate::buffer::Buffer;

fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        return c;
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Knuth-Morris-Pratt matcher over chars.
struct Matcher {
    pattern: Vec<char>,
    fail: Vec<usize>,
    case_sensitive: bool,
}

impl Matcher {
    fn new(query: &str, case_sensitive: bool, reversed: bool) -> Self {
        let mut pattern: Vec<char> = query.chars().map(|c| fold(c, case_sensitive)).collect();
        if reversed {
            pattern.reverse();
        }
        let mut fail = vec![0; pattern.len()];
        let mut k = 0;
        for i in 1..pattern.len() {
            while k > 0 && pattern[i] != pattern[k] {
                k = fail[k - 1];
            }
            if pattern[i] == pattern[k] {
                k += 1;
            }
            fail[i] = k;
        }
        Matcher {
            pattern,
            fail,
            case_sensitive,
        }
    }

    fn len(&self) -> usize {
        self.pattern.len()
    }

    /// Feeds `c`; returns the new state. `state == len()` is a match.
    fn step(&self, mut state: usize, c: char) -> usize {
        if state == self.pattern.len() {
            state = self.fail[state - 1];
        }
        let c = fold(c, self.case_sensitive);
        while state > 0 && self.pattern[state] != c {
            state = self.fail[state - 1];
        }
        if self.pattern[state] == c {
            state + 1
        } else {
            0
        }
    }
}

/// First match starting at or after `from` and ending at or before `limit`.
fn scan_forward(buffer: &Buffer, m: &Matcher, from: usize, limit: usize) -> Option<Range<usize>> {
    let mut state = 0;
    let mut pos = from;
    for c in buffer.text().chars_at(from) {
        if pos >= limit {
            break;
        }
        state = m.step(state, c);
        pos += 1;
        if state == m.len() {
            return Some(pos - m.len()..pos);
        }
    }
    None
}

/// Last match ending at or before `from` and starting at or after `limit`.
/// `m` must be built with `reversed = true`.
fn scan_backward(buffer: &Buffer, m: &Matcher, from: usize, limit: usize) -> Option<Range<usize>> {
    let mut state = 0;
    let mut pos = from;
    let mut chars = buffer.text().chars_at(from);
    while pos > limit {
        let c = chars.prev()?;
        pos -= 1;
        state = m.step(state, c);
        if state == m.len() {
            return Some(pos..pos + m.len());
        }
    }
    None
}

/// Finds the first occurrence of `query` starting at or after `from_char`.
/// With `wrap`, continues from the buffer start. Empty queries never match.
pub fn find_next(
    buffer: &Buffer,
    query: &str,
    from_char: usize,
    case_sensitive: bool,
    wrap: bool,
) -> Option<Range<usize>> {
    if query.is_empty() {
        return None;
    }
    let m = Matcher::new(query, case_sensitive, false);
    let len = buffer.len_chars();
    let from = buffer.clamp_char(from_char);
    scan_forward(buffer, &m, from, len).or_else(|| {
        if wrap {
            let limit = (from + m.len()).saturating_sub(1).min(len);
            scan_forward(buffer, &m, 0, limit)
        } else {
            None
        }
    })
}

/// Finds the last occurrence of `query` ending at or before `from_char`.
/// With `wrap`, continues from the buffer end. Empty queries never match.
pub fn find_prev(
    buffer: &Buffer,
    query: &str,
    from_char: usize,
    case_sensitive: bool,
    wrap: bool,
) -> Option<Range<usize>> {
    if query.is_empty() {
        return None;
    }
    let m = Matcher::new(query, case_sensitive, true);
    let len = buffer.len_chars();
    let from = buffer.clamp_char(from_char);
    scan_backward(buffer, &m, from, 0).or_else(|| {
        if wrap {
            let limit = (from + 1).saturating_sub(m.len());
            scan_backward(buffer, &m, len, limit)
        } else {
            None
        }
    })
}

/// All non-overlapping occurrences of `query`, in order.
pub fn find_all(buffer: &Buffer, query: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let m = Matcher::new(query, case_sensitive, false);
    let mut state = 0;
    for (i, c) in buffer.text().chars().enumerate() {
        state = m.step(state, c);
        if state == m.len() {
            out.push(i + 1 - m.len()..i + 1);
            state = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_wrap() {
        let b = Buffer::new("abc abc ABC");
        assert_eq!(find_next(&b, "abc", 0, true, false), Some(0..3));
        assert_eq!(find_next(&b, "abc", 1, true, false), Some(4..7));
        assert_eq!(find_next(&b, "abc", 5, true, false), None);
        assert_eq!(find_next(&b, "abc", 5, true, true), Some(0..3));
        assert_eq!(find_next(&b, "abc", 5, false, false), Some(8..11));
        assert_eq!(find_next(&b, "abc", 99, false, true), Some(0..3));
        assert_eq!(find_next(&b, "", 0, true, true), None);
        assert_eq!(find_next(&b, "zzz", 0, true, true), None);
    }

    #[test]
    fn prev_and_wrap() {
        let b = Buffer::new("abc abc ABC");
        assert_eq!(find_prev(&b, "abc", 11, true, false), Some(4..7));
        assert_eq!(find_prev(&b, "abc", 6, true, false), Some(0..3));
        assert_eq!(find_prev(&b, "abc", 2, true, false), None);
        assert_eq!(find_prev(&b, "abc", 2, true, true), Some(4..7));
        assert_eq!(find_prev(&b, "abc", 11, false, false), Some(8..11));
        assert_eq!(find_prev(&b, "abc", 2, false, true), Some(8..11));
        assert_eq!(find_prev(&b, "", 2, false, true), None);
    }

    #[test]
    fn overlapping_and_kmp_fallback() {
        let b = Buffer::new("aaaa aabaab");
        assert_eq!(find_next(&b, "aa", 1, true, false), Some(1..3));
        assert_eq!(find_all(&b, "aa", true), vec![0..2, 2..4, 5..7, 8..10]);
        assert_eq!(find_next(&b, "aab", 0, true, false), Some(5..8));
        assert_eq!(find_prev(&b, "aab", 11, true, false), Some(8..11));
        assert_eq!(find_prev(&b, "aa", 4, true, false), Some(2..4));
    }

    #[test]
    fn unicode_and_case_folding() {
        let b = Buffer::new("Ünïcode 😀 ünïCODE 中文");
        assert_eq!(find_all(&b, "ünïcode", false), vec![0..7, 10..17]);
        assert_eq!(find_all(&b, "ünïcode", true), Vec::<Range<usize>>::new());
        assert_eq!(find_next(&b, "😀", 0, true, false), Some(8..9));
        assert_eq!(find_prev(&b, "中文", 21, true, false), Some(18..20));
        assert_eq!(find_all(&b, "\u{301}", true), Vec::<Range<usize>>::new());
    }

    #[test]
    fn multiline_query_and_long_text() {
        let text: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let b = Buffer::new(&text);
        let r = find_next(&b, "19999\n", 0, true, false).expect("found");
        assert_eq!(b.slice_to_string(r.clone()), "19999\n");
        assert_eq!(
            find_prev(&b, "line 0\nline 1\n", b.len_chars(), true, false),
            Some(0..14)
        );
        assert_eq!(find_all(&b, "line 1999\n", true).len(), 1);
    }
}
