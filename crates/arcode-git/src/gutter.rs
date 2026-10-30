//! Combining unstaged and staged hunks into one set of editor gutter marks.

use std::ops::Range;

use crate::{ChangeKind, Hunk};

/// One change shown in the editor gutter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GutterHunk {
    /// Added, modified or deleted.
    pub kind: ChangeKind,
    /// Buffer lines covered (empty for a deletion, starting at the line
    /// after the removed block).
    pub lines: Range<usize>,
    /// True when the change is staged (`HEAD` against the index) and the
    /// buffer has no further changes on these lines.
    pub staged: bool,
    /// The hunk to pass to [`crate::stage_hunk`] / [`crate::revert_hunk`]
    /// (unstaged: index against the buffer) or to [`crate::unstage_hunk`]
    /// (staged: `HEAD` against the index).
    pub hunk: Hunk,
}

impl GutterHunk {
    /// The buffer line the gutter mark is drawn on, given the buffer's line
    /// count: the first changed line, or for a deletion the line above the
    /// removed block (line 0 for a deletion at the top).
    pub fn marker_line(&self, line_count: usize) -> usize {
        let last = line_count.saturating_sub(1);
        match self.kind {
            ChangeKind::Deleted => self.lines.start.saturating_sub(1).min(last),
            _ => self.lines.start.min(last),
        }
    }

    /// True when the gutter mark of this hunk covers `line`.
    pub fn covers(&self, line: usize, line_count: usize) -> bool {
        if self.lines.is_empty() {
            self.marker_line(line_count) == line
        } else {
            self.lines.contains(&line)
        }
    }
}

/// Merges `unstaged` hunks (index against the buffer, from
/// [`crate::diff_against_index`]) with `staged` hunks (`HEAD` against the
/// index, from [`crate::staged_hunks`]) into gutter marks in buffer lines,
/// sorted by line.
///
/// A staged hunk is mapped from index lines to buffer lines through the
/// unstaged hunks. When unstaged changes overlap it, only the unstaged
/// change is shown (its popover offers staging; the staged part is still
/// visible in the source control view).
pub fn gutter_hunks(unstaged: &[Hunk], staged: &[Hunk]) -> Vec<GutterHunk> {
    let mut out: Vec<GutterHunk> = unstaged
        .iter()
        .map(|h| GutterHunk {
            kind: h.kind,
            lines: h.new_range(),
            staged: false,
            hunk: h.clone(),
        })
        .collect();
    for hunk in staged {
        if let Some(start) = map_index_range(unstaged, &hunk.new_range()) {
            out.push(GutterHunk {
                kind: hunk.kind,
                lines: start..start + hunk.new_lines,
                staged: true,
                hunk: hunk.clone(),
            });
        }
    }
    out.sort_by_key(|h| (h.lines.start, h.staged));
    out
}

/// Maps the index line range `range` to its first buffer line, or `None`
/// when an unstaged hunk changes lines inside it.
fn map_index_range(unstaged: &[Hunk], range: &Range<usize>) -> Option<usize> {
    let mut delta: isize = 0;
    for u in unstaged {
        let old = u.old_range();
        let overlaps = if old.is_empty() {
            range.start < old.start && old.start < range.end
        } else if range.is_empty() {
            old.start < range.start && range.start < old.end
        } else {
            old.start < range.end && range.start < old.end
        };
        if overlaps {
            return None;
        }
        if old.end <= range.start {
            delta += u.new_lines as isize - u.old_lines as isize;
        }
    }
    usize::try_from(range.start as isize + delta).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_hunks;

    #[test]
    fn staged_hunks_follow_unstaged_insertions() {
        let head = "a\nb\nc\nd\n";
        let index = "a\nB\nc\nd\n";
        let buffer = "new\na\nB\nc\nd\nmore\n";
        let unstaged = diff_hunks(index, buffer);
        let staged = diff_hunks(head, index);
        let marks = gutter_hunks(&unstaged, &staged);
        let summary: Vec<_> = marks
            .iter()
            .map(|m| (m.kind, m.lines.clone(), m.staged))
            .collect();
        assert_eq!(
            summary,
            [
                (ChangeKind::Added, 0..1, false),
                (ChangeKind::Modified, 2..3, true),
                (ChangeKind::Added, 5..6, false),
            ]
        );
    }

    #[test]
    fn overlapping_unstaged_change_hides_the_staged_one() {
        let head = "a\nb\nc\n";
        let index = "a\nB\nc\n";
        let buffer = "a\nBB\nc\n";
        let marks = gutter_hunks(&diff_hunks(index, buffer), &diff_hunks(head, index));
        assert_eq!(marks.len(), 1);
        assert!(!marks[0].staged);
    }

    #[test]
    fn deletion_marker_sits_above_the_removed_lines() {
        let marks = gutter_hunks(&diff_hunks("a\nb\nc\n", "a\nc\n"), &[]);
        assert_eq!(marks[0].kind, ChangeKind::Deleted);
        assert_eq!(marks[0].marker_line(2), 0);
        assert!(marks[0].covers(0, 2));
        let top = gutter_hunks(&diff_hunks("a\nb\n", "b\n"), &[]);
        assert_eq!(top[0].marker_line(1), 0);
        let staged_delete = gutter_hunks(&[], &diff_hunks("a\nb\nc\n", "a\nc\n"));
        assert_eq!(staged_delete[0].lines, 1..1);
        assert!(staged_delete[0].staged);
    }
}
