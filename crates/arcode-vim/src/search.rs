//! Literal search for `/ ? * # n N` and `:s`.
//!
//! Patterns are matched literally (no regex). A `\c` anywhere in a typed
//! pattern makes the search case-insensitive (and is removed).

use std::ops::Range;

use arcode_buffer::{Buffer, search};

use crate::text::{ch, is_word};
use crate::types::SearchQuery;

/// Builds a query from a typed `/` or `?` pattern.
pub(crate) fn query_from_input(input: &str, forward: bool) -> SearchQuery {
    let case_sensitive = !input.contains("\\c");
    SearchQuery {
        pattern: input.replace("\\c", ""),
        whole_word: false,
        case_sensitive,
        forward,
    }
}

fn word_bounded(b: &Buffer, r: &Range<usize>) -> bool {
    let before = r.start == 0 || !is_word(ch(b, r.start - 1));
    let after = r.end >= b.len_chars() || !is_word(ch(b, r.end));
    before && after
}

/// The next match strictly after `from` (forward) or starting before
/// `from` (backward), wrapping around the buffer.
pub(crate) fn find_match(
    b: &Buffer,
    q: &SearchQuery,
    from: usize,
    forward: bool,
) -> Option<Range<usize>> {
    let len = b.len_chars();
    let mut at = if forward { from + 1 } else { from };
    let mut first: Option<usize> = None;
    // Every iteration finds a new match start; stop once one repeats.
    for _ in 0..=len {
        let r = if forward {
            search::find_next(b, &q.pattern, at, q.case_sensitive, true)?
        } else {
            search::find_prev(b, &q.pattern, at, q.case_sensitive, true)?
        };
        if first == Some(r.start) {
            return None;
        }
        first.get_or_insert(r.start);
        if !q.whole_word || word_bounded(b, &r) {
            return Some(r);
        }
        at = if forward {
            r.start + 1
        } else if r.end > 1 {
            r.end - 1
        } else {
            len
        };
    }
    None
}

/// All matches of `pattern` (literal), optionally ignoring case.
pub(crate) fn all_matches(b: &Buffer, pattern: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    search::find_all(b, pattern, case_sensitive)
}

/// The keyword under or after `pos` on its line (for `*` and `#`).
pub(crate) fn word_under(b: &Buffer, pos: usize) -> Option<Range<usize>> {
    let end_of_line = b.line_end(b.char_to_line(pos));
    let mut start = (pos..end_of_line).find(|&i| is_word(ch(b, i)))?;
    while start > 0 && is_word(ch(b, start - 1)) {
        start -= 1;
    }
    let mut end = start;
    while end < end_of_line && is_word(ch(b, end)) {
        end += 1;
    }
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(p: &str, whole: bool) -> SearchQuery {
        SearchQuery {
            pattern: p.into(),
            whole_word: whole,
            case_sensitive: true,
            forward: true,
        }
    }

    #[test]
    fn matches() {
        let b = Buffer::new("foo foobar foo");
        assert_eq!(find_match(&b, &q("foo", false), 0, true), Some(4..7));
        assert_eq!(find_match(&b, &q("foo", true), 0, true), Some(11..14));
        assert_eq!(find_match(&b, &q("foo", true), 11, true), Some(0..3));
        assert_eq!(find_match(&b, &q("foo", true), 11, false), Some(0..3));
        assert_eq!(find_match(&b, &q("foo", true), 0, false), Some(11..14));
        assert_eq!(find_match(&b, &q("zzz", false), 0, true), None);
        assert_eq!(find_match(&b, &q("foobar", true), 0, true), Some(4..10));
        let only = Buffer::new("xfoox");
        assert_eq!(find_match(&only, &q("foo", true), 0, true), None);
        assert_eq!(word_under(&b, 5), Some(4..10));
        assert_eq!(word_under(&Buffer::new("  ab"), 0), Some(2..4));
        let typed = query_from_input("Ab\\c", false);
        assert!(!typed.case_sensitive && typed.pattern == "Ab");
    }
}
