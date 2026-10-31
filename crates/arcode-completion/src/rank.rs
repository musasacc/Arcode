//! Fuzzy ranking of completion candidates with nucleo-matcher.

use std::cmp::Ordering;
use std::collections::hash_map::Entry as Slot;
use std::collections::{HashMap, HashSet};

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::candidate::{CandidateKind, CandidateSource, CompletionCandidate};

/// One ranked result, referring back into the candidate slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedCandidate {
    /// Index into the slice passed to [`rank`].
    pub index: usize,
    /// Fuzzy score (0 for an empty query); higher is better.
    pub score: u32,
    /// Sorted char indices into the candidate's **label** to highlight.
    /// Empty when the query is empty or only `filter_text` matched.
    pub match_indices: Vec<u32>,
}

/// Reusable ranker; keeps the matcher's scratch memory between calls.
pub struct Ranker {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Default for Ranker {
    fn default() -> Self {
        Ranker::new()
    }
}

impl std::fmt::Debug for Ranker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ranker").finish_non_exhaustive()
    }
}

/// How well a candidate's filter text starts with the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PrefixClass {
    None,
    /// Prefix ignoring case.
    Folded,
    /// Prefix with exact case.
    Exact,
}

struct Entry {
    index: usize,
    prefix: PrefixClass,
    score: u32,
    label_len: usize,
}

impl Ranker {
    /// A ranker configured for completion (prefix-preferring fuzzy match).
    pub fn new() -> Self {
        let mut config = Config::DEFAULT;
        config.prefer_prefix = true;
        Ranker {
            matcher: Matcher::new(config),
            buf: Vec::new(),
        }
    }

    /// Ranks `candidates` for `query`, best first, at most `limit` results.
    ///
    /// Matching is fuzzy with smart case on `filter_text` (or `label`).
    /// Order: exact-prefix match, fuzzy score, LSP `sort_text`, source
    /// priority (LSP > Snippet > BufferWord > Path), shorter label, label,
    /// input order. Within a local source, candidates with a `sort_text`
    /// come first in its order. An empty query keeps every candidate:
    /// preselected first, then `sort_text` order (server items with equal
    /// `sort_text` by label). Duplicates (same label and kind) are dropped,
    /// keeping the best-ranked one, except items of one server: those can
    /// differ only in what they import (`HashMap` from two crates), so they
    /// are kept unless their detail and inserted text are the same too.
    pub fn rank(
        &mut self,
        query: &str,
        candidates: &[CompletionCandidate],
        limit: usize,
    ) -> Vec<RankedCandidate> {
        if limit == 0 {
            return Vec::new();
        }
        if query.trim().is_empty() {
            return rank_empty(candidates, limit);
        }
        let pattern = Pattern::new(
            query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let case_sensitive = query.chars().any(char::is_uppercase);
        let folded_query = (!case_sensitive).then(|| query.to_lowercase());

        let mut entries: Vec<Entry> = Vec::new();
        for (index, cand) in candidates.iter().enumerate() {
            let hay = cand.filter_text();
            let Some(score) = pattern.score(utf32(hay, &mut self.buf), &mut self.matcher) else {
                continue;
            };
            entries.push(Entry {
                index,
                prefix: prefix_class(hay, query, folded_query.as_deref()),
                score,
                label_len: cand.label.chars().count(),
            });
        }
        let cmp = |a: &Entry, b: &Entry| {
            b.prefix
                .cmp(&a.prefix)
                .then(b.score.cmp(&a.score))
                .then_with(|| source_order(&candidates[a.index], &candidates[b.index]))
                .then(a.label_len.cmp(&b.label_len))
                .then_with(|| candidates[a.index].label.cmp(&candidates[b.index].label))
                .then(a.index.cmp(&b.index))
        };
        // Only the head needs full sorting; leave room for dropped duplicates.
        let head = limit.saturating_mul(4).max(64);
        let split = if entries.len() > head {
            entries.select_nth_unstable_by(head, cmp);
            head
        } else {
            entries.len()
        };
        entries[..split].sort_unstable_by(cmp);

        let mut seen = Seen::default();
        let mut out = Vec::new();
        for i in 0..entries.len() {
            if i == split {
                entries[split..].sort_unstable_by(cmp);
            }
            let entry = &entries[i];
            let cand = &candidates[entry.index];
            if !seen.keep(cand) {
                continue;
            }
            let mut match_indices = Vec::new();
            let label = utf32(&cand.label, &mut self.buf);
            if pattern
                .indices(label, &mut self.matcher, &mut match_indices)
                .is_none()
            {
                match_indices.clear();
            }
            match_indices.sort_unstable();
            match_indices.dedup();
            out.push(RankedCandidate {
                index: entry.index,
                score: entry.score,
                match_indices,
            });
            if out.len() == limit {
                break;
            }
        }
        out
    }
}

/// Ranks with a fresh [`Ranker`]; see [`Ranker::rank`].
pub fn rank(query: &str, candidates: &[CompletionCandidate], limit: usize) -> Vec<RankedCandidate> {
    Ranker::new().rank(query, candidates, limit)
}

fn rank_empty(candidates: &[CompletionCandidate], limit: usize) -> Vec<RankedCandidate> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| {
        let (ca, cb) = (&candidates[a], &candidates[b]);
        cb.preselect
            .cmp(&ca.preselect)
            .then_with(|| source_order(ca, cb))
            // Server items with equal `sort_text` by label, like VS Code;
            // local sources keep their own order.
            .then_with(|| match ca.source.is_lsp() && cb.source.is_lsp() {
                true => ca.label.cmp(&cb.label),
                false => Ordering::Equal,
            })
            .then(a.cmp(&b))
    });
    let mut seen = Seen::default();
    order
        .into_iter()
        .filter(|&i| seen.keep(&candidates[i]))
        .take(limit)
        .map(|index| RankedCandidate {
            index,
            score: 0,
            match_indices: Vec::new(),
        })
        .collect()
}

/// What was ranked so far, to drop duplicates: a local candidate (buffer
/// word, path, snippet) duplicates an earlier candidate with the same label
/// and kind; a server item duplicates an earlier local candidate with the
/// same label and kind, or an item of *another* server with the same label
/// and inserted text (servers disagree on kinds). Items of one server are
/// duplicates only when everything shown and inserted is the same (e.g.
/// rust-analyzer's `len()` of `String` and of `str` through `Deref`);
/// otherwise they can differ in what they import.
#[derive(Default)]
struct Seen<'a> {
    /// (label, kind) -> whether the first was local.
    by_kind: HashMap<(&'a str, CandidateKind), bool>,
    /// (label, inserted text) -> server of the first server item.
    by_edit: HashMap<(&'a str, &'a str), &'a str>,
    /// Server items as shown: (label, kind, detail, inserted text).
    shown: HashSet<(&'a str, CandidateKind, Option<&'a str>, &'a str)>,
}

impl<'a> Seen<'a> {
    /// Whether `cand` (ranked after every candidate seen so far) is kept.
    fn keep(&mut self, cand: &'a CompletionCandidate) -> bool {
        let key = (cand.label.as_str(), cand.kind);
        let CandidateSource::Lsp { server } = &cand.source else {
            return match self.by_kind.entry(key) {
                Slot::Vacant(slot) => {
                    slot.insert(true);
                    true
                }
                Slot::Occupied(_) => false,
            };
        };
        if self.by_kind.get(&key) == Some(&true) {
            return false;
        }
        let shown = (key.0, key.1, cand.detail.as_deref(), cand.insert.text());
        if !self.shown.insert(shown) {
            return false;
        }
        match self
            .by_edit
            .entry((cand.label.as_str(), cand.insert.text()))
        {
            Slot::Occupied(first) if *first.get() != server.as_str() => return false,
            Slot::Occupied(_) => {}
            Slot::Vacant(slot) => {
                slot.insert(server.as_str());
            }
        }
        self.by_kind.entry(key).or_insert(false);
        true
    }
}

/// `s` as one UTF-32 element per char, so match indices are char indices.
/// (`Utf32Str::new` segments graphemes, or bytes for ASCII-led clusters.)
fn utf32<'a>(s: &'a str, buf: &'a mut Vec<char>) -> Utf32Str<'a> {
    if s.is_ascii() {
        Utf32Str::Ascii(s.as_bytes())
    } else {
        buf.clear();
        buf.extend(s.chars());
        Utf32Str::Unicode(buf)
    }
}

fn prefix_class(hay: &str, query: &str, folded_query: Option<&str>) -> PrefixClass {
    if hay.starts_with(query) {
        return PrefixClass::Exact;
    }
    if let Some(folded) = folded_query {
        let mut hay_chars = hay.chars().flat_map(char::to_lowercase);
        if folded.chars().all(|q| hay_chars.next() == Some(q)) {
            return PrefixClass::Folded;
        }
    }
    PrefixClass::None
}

/// `sort_text` order within one source, else source priority.
fn source_order(a: &CompletionCandidate, b: &CompletionCandidate) -> Ordering {
    let (pa, pb) = (a.source.priority(), b.source.priority());
    if pa != pb {
        return pb.cmp(&pa);
    }
    if a.source.is_lsp() {
        a.sort_text().cmp(b.sort_text())
    } else {
        // Local sources: those with a sort_text first, in its order. Must
        // stay a total order (sorting may panic otherwise).
        match (&a.sort_text, &b.sort_text) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

/// Merges candidates with the same label and kind (e.g. from several
/// servers), keeping the best: higher source priority, then preselected,
/// then one with a detail, then the earliest. Order of first appearance is
/// preserved.
pub fn merge_duplicates(candidates: Vec<CompletionCandidate>) -> Vec<CompletionCandidate> {
    let mut out: Vec<CompletionCandidate> = Vec::with_capacity(candidates.len());
    let mut slot: std::collections::HashMap<(String, CandidateKind), usize> =
        std::collections::HashMap::new();
    for cand in candidates {
        let key = (cand.label.clone(), cand.kind);
        match slot.get(&key) {
            Some(&i) => {
                if quality(&cand) > quality(&out[i]) {
                    out[i] = cand;
                }
            }
            None => {
                slot.insert(key, out.len());
                out.push(cand);
            }
        }
    }
    out
}

fn quality(c: &CompletionCandidate) -> (u8, bool, bool) {
    (c.source.priority(), c.preselect, c.detail.is_some())
}

#[cfg(test)]
#[path = "rank_tests.rs"]
mod tests;
