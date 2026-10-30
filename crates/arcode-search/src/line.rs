//! Building [`LineMatch`]es: byte ranges → char columns, and truncation of
//! long lines around the first match.

use std::ops::Range;

use grep_matcher::Matcher;

/// Lines longer than this many chars are shortened around the first match.
pub const MAX_LINE_CHARS: usize = 400;
/// Chars kept before the first match when a line is shortened.
const LEAD_CHARS: usize = 60;
/// Marker for text cut from either side of a shortened line.
pub const ELLIPSIS: char = '…';

/// One line containing at least one match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// Zero-based line number.
    pub line: usize,
    /// The line without its terminator. Lines over [`MAX_LINE_CHARS`] are
    /// cut around the first match, with [`ELLIPSIS`] marking each cut side.
    pub line_text: String,
    /// Char ranges of the matches inside `line_text` (for highlighting).
    pub ranges: Vec<Range<usize>>,
    /// Char column ranges of the matches in the original, uncut line (for
    /// moving the cursor). Equal to `ranges` when the line was not cut.
    pub columns: Vec<Range<usize>>,
}

impl LineMatch {
    /// Find all non-empty matches of `matcher` in `raw` (one line, with or
    /// without its terminator), keeping at most `limit` of them. `None` when
    /// nothing (non-empty) matches.
    pub(crate) fn build<M: Matcher>(
        matcher: &M,
        line: usize,
        raw: &[u8],
        limit: usize,
    ) -> Option<Self> {
        let text = String::from_utf8_lossy(trim_terminator(raw));
        let columns = char_ranges(matcher, &text, limit);
        if columns.is_empty() {
            return None;
        }
        let (line_text, ranges) = truncate(&text, &columns);
        Some(Self {
            line,
            line_text,
            ranges,
            columns,
        })
    }

    /// Number of matches on this line (including any cut from `line_text`).
    pub fn match_count(&self) -> usize {
        self.columns.len()
    }

    /// Keep only the first `n` matches. `ranges[i]` always maps
    /// `columns[i]`: the cut window starts at or before the first match, so
    /// visible ranges are a prefix of the columns.
    pub(crate) fn truncate_matches(&mut self, n: usize) {
        self.columns.truncate(n);
        self.ranges.truncate(n);
    }
}

/// Strip a trailing `\n` or `\r\n`.
pub(crate) fn trim_terminator(raw: &[u8]) -> &[u8] {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    raw.strip_suffix(b"\r").unwrap_or(raw)
}

/// Char ranges of the first `limit` non-empty matches in `text`.
fn char_ranges<M: Matcher>(matcher: &M, text: &str, limit: usize) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let (mut byte_pos, mut char_pos) = (0usize, 0usize);
    let mut to_char = |b: usize| {
        char_pos += count_chars(&bytes[byte_pos.min(b)..b]);
        byte_pos = b;
        char_pos
    };
    let result = matcher.find_iter(bytes, |m| {
        if !m.is_empty() {
            let start = to_char(m.start());
            let end = to_char(m.end());
            ranges.push(start..end);
        }
        ranges.len() < limit
    });
    if let Err(err) = result {
        log::debug!("search: matcher error: {err}");
    }
    ranges
}

/// Count chars in a UTF-8 byte slice that may start or end mid-char.
fn count_chars(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| (b & 0xC0) != 0x80).count()
}

/// Cut `text` to at most [`MAX_LINE_CHARS`] (plus ellipses) around the first
/// match and translate `columns` into the cut text.
fn truncate(text: &str, columns: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let total = text.chars().count();
    if total <= MAX_LINE_CHARS {
        return (text.to_owned(), columns.to_vec());
    }
    let first = columns.first().map_or(0, |r| r.start);
    let mut start = first.saturating_sub(LEAD_CHARS);
    let end = (start + MAX_LINE_CHARS).min(total);
    start = end.saturating_sub(MAX_LINE_CHARS);

    let mut out = String::with_capacity(MAX_LINE_CHARS + 8);
    let prefix = usize::from(start > 0);
    if start > 0 {
        out.push(ELLIPSIS);
    }
    out.extend(text.chars().skip(start).take(end - start));
    if end < total {
        out.push(ELLIPSIS);
    }
    let ranges = columns
        .iter()
        .filter_map(|r| {
            let (s, e) = (r.start.max(start), r.end.min(end));
            (s < e).then(|| s - start + prefix..e - start + prefix)
        })
        .collect();
    (out, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_regex::RegexMatcher;

    fn chars(s: &str, r: &Range<usize>) -> String {
        s.chars().skip(r.start).take(r.end - r.start).collect()
    }

    #[test]
    fn unicode_columns_are_chars() {
        let m = RegexMatcher::new("wörld").unwrap();
        let lm = LineMatch::build(&m, 3, "héllo wörld, wörld\r\n".as_bytes(), usize::MAX).unwrap();
        assert_eq!(lm.line_text, "héllo wörld, wörld");
        assert_eq!(lm.ranges, vec![6..11, 13..18]);
        assert_eq!(lm.columns, lm.ranges);
        assert_eq!(lm.line, 3);
    }

    #[test]
    fn empty_matches_are_skipped() {
        let m = RegexMatcher::new("x*").unwrap();
        assert!(LineMatch::build(&m, 0, b"abc\n", usize::MAX).is_none());
        let lm = LineMatch::build(&m, 0, b"axxb", usize::MAX).unwrap();
        assert_eq!(lm.ranges, vec![1..3]);
    }

    #[test]
    fn long_line_is_cut_around_match() {
        let line = format!("{}NEEDLE{}", "é".repeat(1000), "b".repeat(1000));
        let m = RegexMatcher::new("NEEDLE").unwrap();
        let lm = LineMatch::build(&m, 0, line.as_bytes(), usize::MAX).unwrap();
        assert_eq!(lm.columns, vec![1000..1006]);
        assert_eq!(lm.line_text.chars().count(), MAX_LINE_CHARS + 2);
        assert!(lm.line_text.starts_with(ELLIPSIS) && lm.line_text.ends_with(ELLIPSIS));
        assert_eq!(chars(&lm.line_text, &lm.ranges[0]), "NEEDLE");
    }

    #[test]
    fn cut_near_end_keeps_full_window() {
        let line = format!("{}END", "a".repeat(900));
        let m = RegexMatcher::new("END").unwrap();
        let lm = LineMatch::build(&m, 0, line.as_bytes(), usize::MAX).unwrap();
        assert!(lm.line_text.starts_with(ELLIPSIS));
        assert!(lm.line_text.ends_with("END"));
        assert_eq!(lm.line_text.chars().count(), MAX_LINE_CHARS + 1);
        assert_eq!(chars(&lm.line_text, &lm.ranges[0]), "END");
    }

    #[test]
    fn matches_outside_window_are_dropped_from_ranges() {
        let line = format!("X{}X", "a".repeat(2000));
        let m = RegexMatcher::new("X").unwrap();
        let lm = LineMatch::build(&m, 0, line.as_bytes(), usize::MAX).unwrap();
        assert_eq!(lm.columns.len(), 2);
        assert_eq!(lm.ranges, vec![0..1]);
        assert!(!lm.line_text.starts_with(ELLIPSIS));
    }

    #[test]
    fn truncate_matches_keeps_ranges_aligned() {
        let line = format!("{}X X{}X", "a".repeat(500), "b".repeat(2000));
        let m = RegexMatcher::new("X").unwrap();
        let mut lm = LineMatch::build(&m, 0, line.as_bytes(), usize::MAX).unwrap();
        assert_eq!((lm.columns.len(), lm.ranges.len()), (3, 2));
        lm.truncate_matches(1);
        assert_eq!(lm.columns, vec![500..501]);
        assert_eq!(lm.ranges.len(), 1);
        assert_eq!(chars(&lm.line_text, &lm.ranges[0]), "X");
    }
}
