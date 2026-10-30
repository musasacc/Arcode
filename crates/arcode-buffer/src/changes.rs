//! Normalized change lists (pre-edit char ranges) and position mapping.

use std::borrow::Cow;
use std::ops::Range;

use crate::line_ending::normalize;

/// One change in pre-edit char coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub range: Range<usize>,
    pub text: String,
    /// Length of `text` in chars.
    pub text_chars: usize,
}

/// Which side a position sticks to when text is inserted exactly at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Assoc {
    /// Stay before inserted text.
    Before,
    /// Move after inserted text.
    After,
}

/// Sorts, clamps and de-overlaps changes; drops no-ops.
///
/// Each range is clamped to `0..=len` (an inverted range becomes empty at
/// its start). Ranges are sorted by `(start, end)`; ties keep input
/// order. Overlapping ranges are merged into their union with texts
/// concatenated in sorted order, so the result is always non-overlapping.
pub(crate) fn normalize_changes(edits: Vec<(Range<usize>, String)>, len: usize) -> Vec<Change> {
    let mut items: Vec<(Range<usize>, String)> = edits
        .into_iter()
        .map(|(r, t)| {
            let s = r.start.min(len);
            let e = r.end.min(len).max(s);
            let text = match normalize(&t) {
                Cow::Borrowed(_) => t,
                Cow::Owned(o) => o,
            };
            (s..e, text)
        })
        .collect();
    items.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out: Vec<Change> = Vec::with_capacity(items.len());
    for (range, text) in items {
        if let Some(last) = out.last_mut()
            && range.start < last.range.end
        {
            last.range.end = last.range.end.max(range.end);
            last.text_chars += text.chars().count();
            last.text.push_str(&text);
            continue;
        }
        let text_chars = text.chars().count();
        out.push(Change {
            range,
            text,
            text_chars,
        });
    }
    out.retain(|c| !(c.range.is_empty() && c.text.is_empty()));
    out
}

/// Maps a pre-edit position through sorted, non-overlapping `changes`.
///
/// Positions inside a replaced range move to its start (`Before`) or to the
/// end of the replacement (`After`); the end of a replaced range always maps
/// to the end of its replacement. O(number of changes); use [`ChangeMap`]
/// to map many positions.
#[cfg(test)]
pub(crate) fn map_pos(pos: usize, changes: &[Change], assoc: Assoc) -> usize {
    map_from(pos, changes, assoc, 0)
}

/// [`map_pos`] over `changes` (a suffix of a change list) with `delta` the
/// accumulated length change of the changes before them.
fn map_from(pos: usize, changes: &[Change], assoc: Assoc, mut delta: isize) -> usize {
    for c in changes {
        let (s, e) = (c.range.start, c.range.end);
        let shifted = |p: usize| (p as isize + delta) as usize;
        if pos < s || (pos == s && assoc == Assoc::Before && s < e) {
            return shifted(pos);
        }
        if pos == s && s == e {
            if assoc == Assoc::Before {
                return shifted(s);
            }
            // After: also skip any further insertions at the same spot.
            delta += c.text_chars as isize;
            continue;
        }
        if pos < e {
            return match assoc {
                Assoc::Before => shifted(s),
                Assoc::After => shifted(s) + c.text_chars,
            };
        }
        if pos == e {
            return shifted(s) + c.text_chars;
        }
        delta += c.text_chars as isize - (e - s) as isize;
    }
    (pos as isize + delta) as usize
}

/// Maps many positions through one change list in O(log k) each (plus the
/// changes sitting exactly at the position), instead of O(k).
pub(crate) struct ChangeMap<'a> {
    changes: &'a [Change],
    /// `prefix[i]` = total length change of `changes[..i]`.
    prefix: Vec<isize>,
}

impl<'a> ChangeMap<'a> {
    /// Precomputes prefix sums for sorted, non-overlapping `changes`.
    pub(crate) fn new(changes: &'a [Change]) -> Self {
        let mut prefix = Vec::with_capacity(changes.len() + 1);
        let mut acc = 0isize;
        prefix.push(acc);
        for c in changes {
            acc += c.text_chars as isize - c.range.len() as isize;
            prefix.push(acc);
        }
        ChangeMap { changes, prefix }
    }

    /// Same result as [`map_pos`].
    pub(crate) fn map(&self, pos: usize, assoc: Assoc) -> usize {
        // Changes ending strictly before `pos` only shift it; their ends are
        // sorted because the ranges are sorted and non-overlapping.
        let i = self.changes.partition_point(|c| c.range.end < pos);
        map_from(pos, &self.changes[i..], assoc, self.prefix[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(edits: &[(Range<usize>, &str)], len: usize) -> Vec<Change> {
        normalize_changes(
            edits
                .iter()
                .map(|(r, t)| (r.clone(), t.to_string()))
                .collect(),
            len,
        )
    }

    #[test]
    fn sorts_clamps_and_drops_noops() {
        let c = ch(&[(5..6, "x"), (0..1, "y"), (3..3, ""), (8..20, "")], 10);
        let ranges: Vec<_> = c.iter().map(|c| c.range.clone()).collect();
        assert_eq!(ranges, vec![0..1, 5..6, 8..10]);
    }

    #[test]
    fn merges_overlaps_and_keeps_tie_order() {
        let c = ch(&[(2..5, "a"), (4..7, "b")], 10);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].range, 2..7);
        assert_eq!(c[0].text, "ab");
        let c = ch(&[(3..3, "1"), (3..3, "2"), (3..4, "R")], 10);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].text, "1");
        assert_eq!(c[1].text, "2");
        assert_eq!(c[2].text, "R");
    }

    #[test]
    fn normalizes_inserted_line_endings() {
        let c = ch(&[(0..0, "a\r\nb")], 0);
        assert_eq!(c[0].text, "a\nb");
        assert_eq!(c[0].text_chars, 3);
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn inverted_range_becomes_empty() {
        let c = ch(&[(5..2, "x")], 10);
        assert_eq!(c[0].range, 5..5);
    }

    #[test]
    fn mapping() {
        // Insert "XY" at 2, replace 5..7 with "z".
        let c = ch(&[(2..2, "XY"), (5..7, "z")], 10);
        assert_eq!(map_pos(1, &c, Assoc::After), 1);
        assert_eq!(map_pos(2, &c, Assoc::Before), 2);
        assert_eq!(map_pos(2, &c, Assoc::After), 4);
        assert_eq!(map_pos(4, &c, Assoc::After), 6);
        assert_eq!(map_pos(5, &c, Assoc::Before), 7);
        assert_eq!(map_pos(5, &c, Assoc::After), 8);
        assert_eq!(map_pos(6, &c, Assoc::Before), 7);
        assert_eq!(map_pos(6, &c, Assoc::After), 8);
        assert_eq!(map_pos(7, &c, Assoc::Before), 8);
        assert_eq!(map_pos(9, &c, Assoc::Before), 10);
    }

    #[test]
    fn change_map_agrees_with_map_pos() {
        let c = ch(
            &[
                (2..2, "XY"),
                (2..2, "q"),
                (2..4, ""),
                (5..7, "z"),
                (7..7, "abc"),
                (9..12, "\n"),
                (15..15, "w"),
            ],
            20,
        );
        let m = ChangeMap::new(&c);
        for pos in 0..=20 {
            for assoc in [Assoc::Before, Assoc::After] {
                assert_eq!(
                    m.map(pos, assoc),
                    map_pos(pos, &c, assoc),
                    "{pos} {assoc:?}"
                );
            }
        }
        let empty = ChangeMap::new(&[]);
        assert_eq!(empty.map(7, Assoc::After), 7);
    }

    #[test]
    fn mapping_through_stacked_insertions() {
        let c = ch(&[(3..3, "ab"), (3..3, "c"), (3..5, "")], 10);
        assert_eq!(map_pos(3, &c, Assoc::Before), 3);
        assert_eq!(map_pos(3, &c, Assoc::After), 6);
        assert_eq!(map_pos(4, &c, Assoc::After), 6);
        assert_eq!(map_pos(5, &c, Assoc::After), 6);
        assert_eq!(map_pos(8, &c, Assoc::After), 9);
    }
}
