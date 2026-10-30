//! Turns overlapping query captures into flat, non-overlapping spans.

use std::ops::Range;

/// One query capture in absolute byte offsets.
#[derive(Debug, Clone)]
pub(crate) struct Capture {
    /// Byte range of the captured node.
    pub range: Range<usize>,
    /// Index of the query pattern that produced it (later wins on ties).
    pub pattern: usize,
    /// Capture name.
    pub name: &'static str,
}

/// Flattens captures into sorted, non-overlapping spans clipped to `window`.
///
/// Inner captures split outer ones. Captures with the same range are ordered
/// by pattern index, so the later pattern wins. Adjacent spans with the same
/// name are merged. Partially overlapping captures (which tree nodes never
/// produce) are clipped to their enclosing capture.
pub(crate) fn resolve_captures(
    mut captures: Vec<Capture>,
    window: Range<usize>,
) -> Vec<(Range<usize>, &'static str)> {
    captures.retain(|c| c.range.start < c.range.end);
    // Pre-order: outer before inner, earlier pattern before later.
    captures.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
            .then(a.pattern.cmp(&b.pattern))
    });

    let mut out = Out {
        spans: Vec::new(),
        window,
    };
    // Open captures: (end, name); each is nested in the one below it.
    let mut stack: Vec<(usize, &'static str)> = Vec::new();
    let mut pos = 0;
    for capture in captures {
        let Range { start, end } = capture.range;
        while let Some(&(top_end, name)) = stack.last() {
            if top_end > start {
                break;
            }
            out.emit(pos, top_end, name);
            pos = pos.max(top_end);
            stack.pop();
        }
        if let Some(&(_, name)) = stack.last() {
            out.emit(pos, start, name);
        }
        pos = pos.max(start);
        let end = stack.last().map_or(end, |&(top_end, _)| end.min(top_end));
        stack.push((end, capture.name));
    }
    while let Some((end, name)) = stack.pop() {
        out.emit(pos, end, name);
        pos = pos.max(end);
    }
    out.spans
}

struct Out {
    spans: Vec<(Range<usize>, &'static str)>,
    window: Range<usize>,
}

impl Out {
    fn emit(&mut self, start: usize, end: usize, name: &'static str) {
        let start = start.max(self.window.start);
        let end = end.min(self.window.end);
        if start >= end {
            return;
        }
        if let Some((last, last_name)) = self.spans.last_mut()
            && last.end == start
            && *last_name == name
        {
            last.end = end;
            return;
        }
        self.spans.push((start..end, name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(range: Range<usize>, pattern: usize, name: &'static str) -> Capture {
        Capture {
            range,
            pattern,
            name,
        }
    }

    #[test]
    fn inner_splits_outer() {
        let spans = resolve_captures(
            vec![cap(0..10, 5, "string"), cap(3..5, 1, "string.escape")],
            0..100,
        );
        assert_eq!(
            spans,
            vec![(0..3, "string"), (3..5, "string.escape"), (5..10, "string")]
        );
    }

    #[test]
    fn later_pattern_wins_on_same_range() {
        let spans = resolve_captures(
            vec![cap(0..4, 2, "function"), cap(0..4, 1, "constructor")],
            0..100,
        );
        assert_eq!(spans, vec![(0..4, "function")]);
    }

    #[test]
    fn disjoint_and_nested_mix() {
        let spans = resolve_captures(
            vec![
                cap(20..30, 0, "c"),
                cap(0..15, 0, "a"),
                cap(2..4, 0, "b"),
                cap(6..8, 0, "b"),
                cap(6..7, 1, "d"),
            ],
            0..100,
        );
        assert_eq!(
            spans,
            vec![
                (0..2, "a"),
                (2..4, "b"),
                (4..6, "a"),
                (6..7, "d"),
                (7..8, "b"),
                (8..15, "a"),
                (20..30, "c"),
            ]
        );
    }

    #[test]
    fn clips_to_window_and_drops_empty() {
        let spans = resolve_captures(
            vec![
                cap(0..100, 0, "comment"),
                cap(50..50, 1, "x"),
                cap(60..70, 1, "y"),
            ],
            40..65,
        );
        assert_eq!(spans, vec![(40..60, "comment"), (60..65, "y")]);
    }

    #[test]
    fn merges_adjacent_same_name() {
        let spans = resolve_captures(vec![cap(0..2, 0, "k"), cap(2..4, 0, "k")], 0..10);
        assert_eq!(spans, vec![(0..4, "k")]);
    }

    #[test]
    fn partial_overlap_is_clipped() {
        let spans = resolve_captures(vec![cap(0..5, 0, "a"), cap(3..8, 1, "b")], 0..10);
        assert_eq!(spans, vec![(0..3, "a"), (3..5, "b")]);
    }
}
