//! [`LineReplacer`]: a query compiled once, applied line by line to find
//! each match and the text it would be replaced by.

use std::ops::Range;

use grep_matcher::{Captures, Matcher};
use grep_regex::RegexMatcher;

use crate::error::{Result, SearchError};
use crate::query::SearchQuery;

/// Finds the replacements of a query on single lines without compiling the
/// query again for every line (editors replacing thousands of matches, or
/// a result list previewing many lines).
#[derive(Clone, Debug)]
pub struct LineReplacer {
    matcher: RegexMatcher,
    is_regex: bool,
    replacement: String,
}

impl LineReplacer {
    /// Compiles `query`; fails like [`SearchQuery::validate`].
    pub fn new(query: &SearchQuery, replacement: &str) -> Result<Self> {
        Ok(Self {
            matcher: query.matcher()?,
            is_regex: query.is_regex,
            replacement: replacement.to_owned(),
        })
    }

    /// The non-empty matches in `line` (no terminator) with the text each
    /// would be replaced by (regex groups such as `$1` expanded): char
    /// ranges within the line, in order.
    pub fn replacements(&self, line: &str) -> Result<Vec<(Range<usize>, String)>> {
        let matcher = &self.matcher;
        let hay = line.as_bytes();
        let mut caps = matcher
            .new_captures()
            .map_err(|e| SearchError::InvalidPattern(e.to_string()))?;
        let mut out = Vec::new();
        let (mut byte_pos, mut char_pos) = (0usize, 0usize);
        let mut to_char = |b: usize| {
            char_pos += line[byte_pos.min(b)..b].chars().count();
            byte_pos = b;
            char_pos
        };
        let result = matcher.captures_iter(hay, &mut caps, |caps| {
            let Some(m) = caps.get(0) else { return true };
            if m.is_empty() {
                return true;
            }
            let mut dst = Vec::new();
            if self.is_regex {
                caps.interpolate(
                    |name| matcher.capture_index(name),
                    hay,
                    self.replacement.as_bytes(),
                    &mut dst,
                );
            } else {
                dst.extend_from_slice(self.replacement.as_bytes());
            }
            let start = to_char(m.start());
            let end = to_char(m.end());
            out.push((start..end, String::from_utf8_lossy(&dst).into_owned()));
            true
        });
        result.map_err(|e| SearchError::InvalidPattern(e.to_string()))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_compiled_query_serves_many_lines() {
        let q = SearchQuery {
            is_regex: true,
            ..SearchQuery::new(r"(\w)(\d)")
        };
        let r = LineReplacer::new(&q, "$2$1").unwrap();
        assert_eq!(
            r.replacements("äa1 b2").unwrap(),
            vec![(1..3, "1a".to_owned()), (4..6, "2b".to_owned())]
        );
        assert_eq!(r.replacements("none").unwrap(), Vec::new());
        let literal = LineReplacer::new(&SearchQuery::new("a"), "$1").unwrap();
        assert_eq!(
            literal.replacements("bab").unwrap(),
            vec![(1..2, "$1".to_owned())]
        );
        assert!(LineReplacer::new(&SearchQuery::new(""), "x").is_err());
    }
}
