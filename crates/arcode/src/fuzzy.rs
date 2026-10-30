//! Fuzzy ranking of project file paths (nucleo), used by the file finder.

use std::ops::Range;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// One ranked result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatch {
    /// Index into the candidate list.
    pub index: usize,
    /// Byte ranges of the path that matched the query (sorted, merged).
    pub highlights: Vec<Range<usize>>,
}

/// Ranks `paths` against `query`, best first, keeping at most `limit`.
/// An empty query returns the first `limit` paths unhighlighted. Ties are
/// broken by shorter path, then alphabetically.
pub fn rank(paths: &[String], query: &str, limit: usize) -> Vec<FileMatch> {
    let query = query.trim();
    if query.is_empty() {
        return (0..paths.len().min(limit))
            .map(|index| FileMatch {
                index,
                highlights: Vec::new(),
            })
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = paths
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            pattern
                .score(Utf32Str::new(path, &mut buf), &mut matcher)
                .map(|score| (score, i))
        })
        .collect();
    scored.sort_unstable_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| paths[a.1].len().cmp(&paths[b.1].len()))
            .then_with(|| paths[a.1].cmp(&paths[b.1]))
    });
    scored.truncate(limit);
    let mut indices = Vec::new();
    scored
        .into_iter()
        .map(|(_, index)| {
            let path = &paths[index];
            indices.clear();
            pattern.indices(Utf32Str::new(path, &mut buf), &mut matcher, &mut indices);
            indices.sort_unstable();
            indices.dedup();
            FileMatch {
                index,
                highlights: char_indices_to_ranges(path, &indices),
            }
        })
        .collect()
}

/// Converts sorted char positions into merged byte ranges of `text`.
fn char_indices_to_ranges(text: &str, positions: &[u32]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = positions.iter().copied().peekable();
    for (char_ix, (byte, ch)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else { break };
        if next as usize != char_ix {
            continue;
        }
        wanted.next();
        let end = byte + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == byte => last.end = end,
            _ => ranges.push(byte..end),
        }
    }
    ranges
}

/// The highlight ranges inside `text[offset..]`, shifted to start at 0.
pub fn ranges_within(ranges: &[Range<usize>], offset: usize, len: usize) -> Vec<Range<usize>> {
    ranges
        .iter()
        .filter_map(|r| {
            let start = r.start.max(offset);
            let end = r.end.min(offset + len);
            (start < end).then(|| start - offset..end - offset)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ranks_file_name_matches_first() {
        let files = paths(&[
            "crates/arcode-buffer/src/lib.rs",
            "docs/UI_SPEC.md",
            "crates/arcode-ui-kit/src/lib.rs",
            "crates/arcode/src/main.rs",
        ]);
        let result = rank(&files, "main", 10);
        assert_eq!(result.first().map(|m| m.index), Some(3));
        let result = rank(&files, "uispec", 10);
        assert_eq!(result.first().map(|m| m.index), Some(1));
        assert!(rank(&files, "zzzz", 10).is_empty());
    }

    #[test]
    fn empty_query_lists_in_order_and_respects_limit() {
        let files = paths(&["a", "b", "c"]);
        let result = rank(&files, "  ", 2);
        assert_eq!(result.iter().map(|m| m.index).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn highlights_are_byte_ranges() {
        let files = paths(&["src/main.rs"]);
        let result = rank(&files, "main", 10);
        assert_eq!(result[0].highlights, vec![4..8]);
        assert_eq!(char_indices_to_ranges("äbc", &[1, 2]), vec![2..4]);
        assert_eq!(ranges_within(&[2..6], 4, 10), vec![0..2]);
    }
}
