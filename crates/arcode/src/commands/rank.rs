//! Ordering the palette: fuzzy matching with nucleo, recently used first.

use std::ops::Range;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::fuzzy::char_indices_to_ranges;

/// A command as the ranking sees it.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    /// `"Category: Name"`.
    pub label: &'a str,
    /// Position in the recently-used list (0 = most recent), if any.
    pub recent: Option<usize>,
}

/// One row of the filtered list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    /// Index into the candidates.
    pub index: usize,
    /// Byte ranges of the label that matched the query.
    pub highlights: Vec<Range<usize>>,
    /// The command was used recently (listed in the top group).
    pub recent: bool,
}

/// Recent position, score, candidate index and highlight ranges.
type Scored = (Option<usize>, u32, usize, Vec<Range<usize>>);

/// Share of the best score a recent command needs to stay on top.
pub const RECENT_MIN_SCORE: f32 = 0.7;

/// Filters and orders `candidates` for `query`.
///
/// Recently used commands that match well (at least
/// [`RECENT_MIN_SCORE`] of the best score) come first, most recent first;
/// the others follow by match score (best first), then label. An empty
/// query lists everything: recent ones, then the rest alphabetically.
pub fn rank_commands(candidates: &[Candidate<'_>], query: &str) -> Vec<Ranked> {
    let query = query.trim();
    let mut scored: Vec<Scored> = if query.is_empty() {
        candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (c.recent, 0, i, Vec::new()))
            .collect()
    } else {
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let mut indices = Vec::new();
        candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let score = pattern.score(Utf32Str::new(c.label, &mut buf), &mut matcher)?;
                indices.clear();
                pattern.indices(Utf32Str::new(c.label, &mut buf), &mut matcher, &mut indices);
                indices.sort_unstable();
                indices.dedup();
                let ranges = char_indices_to_ranges(c.label, &indices);
                Some((c.recent, score, i, ranges))
            })
            .collect()
    };
    let best = scored.iter().map(|s| s.1).max().unwrap_or(0);
    let floor = (best as f32 * RECENT_MIN_SCORE) as u32;
    for entry in &mut scored {
        if entry.1 < floor {
            entry.0 = None;
        }
    }
    scored.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => {
            b.1.cmp(&a.1)
                .then_with(|| candidates[a.2].label.cmp(candidates[b.2].label))
        }
    });
    scored
        .into_iter()
        .map(|(recent, _, index, highlights)| Ranked {
            index,
            highlights,
            recent: recent.is_some(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands<'a>(labels: &[&'a str], recent: &[&str]) -> Vec<Candidate<'a>> {
        labels
            .iter()
            .map(|l| Candidate {
                label: l,
                recent: recent.iter().position(|r| r == l),
            })
            .collect()
    }

    fn labels<'a>(c: &[Candidate<'a>], ranked: &[Ranked]) -> Vec<&'a str> {
        ranked.iter().map(|r| c[r.index].label).collect()
    }

    #[test]
    fn empty_query_lists_recent_then_alphabetical() {
        let c = cands(
            &["View: Toggle Panel", "Git: Commit", "File: Find File"],
            &["View: Toggle Panel"],
        );
        let ranked = rank_commands(&c, "");
        assert_eq!(
            labels(&c, &ranked),
            ["View: Toggle Panel", "File: Find File", "Git: Commit"]
        );
        assert!(ranked[0].recent && !ranked[1].recent);
    }

    #[test]
    fn query_filters_fuzzily_and_keeps_recent_on_top() {
        let c = cands(
            &[
                "Git: Toggle Blame",
                "View: Toggle Panel",
                "View: Toggle Sidebar",
                "Git: Commit",
            ],
            &["View: Toggle Sidebar"],
        );
        let ranked = rank_commands(&c, "tog");
        let got = labels(&c, &ranked);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], "View: Toggle Sidebar");
        assert!(!got.contains(&"Git: Commit"));
        let ranked = rank_commands(&c, "gitcom");
        assert_eq!(labels(&c, &ranked), ["Git: Commit"]);
        assert!(!ranked[0].highlights.is_empty());
        assert!(rank_commands(&c, "zzzq").is_empty());
    }

    #[test]
    fn weak_matches_of_recent_commands_do_not_jump_ahead() {
        let c = cands(
            &["Editor: Toggle Line Comment", "Git: Commit", "Git: Refresh"],
            &["Editor: Toggle Line Comment"],
        );
        let ranked = rank_commands(&c, "git");
        assert_eq!(c[ranked[0].index].label.split(':').next(), Some("Git"));
        assert!(!ranked.iter().any(|r| r.recent));
    }

    #[test]
    fn better_matches_rank_higher() {
        let c = cands(&["Terminal: New Terminal", "File: New Project"], &[]);
        let ranked = rank_commands(&c, "new project");
        assert_eq!(c[ranked[0].index].label, "File: New Project");
    }
}
