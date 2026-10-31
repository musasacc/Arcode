//! Undo/redo history.

use crate::buffer::Buffer;
use crate::edit::Edit;
use crate::selection::{Selection, SelectionSet};

/// How an edit joins the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UndoGroup {
    /// Start a new undo step.
    #[default]
    New,
    /// Merge into the previous step if it was also `Merge`, nothing (undo,
    /// redo, save, [`Buffer::break_undo_group`]) happened in between and,
    /// when selections are recorded, the new step starts where the previous
    /// one left the selections. Used for typing.
    Merge,
}

/// The result of [`Buffer::undo`] / [`Buffer::redo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoResult {
    /// The applied changes, in application order.
    pub edits: Vec<Edit>,
    /// Selections to restore: those active before the undone transaction
    /// (undo) or after the redone one (redo). When none were recorded,
    /// cursors are placed at the end of each changed range.
    pub selections: SelectionSet,
}

/// One applied change, enough to invert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppliedEdit {
    pub start: usize,
    pub old_text: String,
    pub new_text: String,
    pub old_chars: usize,
    pub new_chars: usize,
}

impl AppliedEdit {
    pub(crate) fn new(start: usize, old_text: String, new_text: String) -> Self {
        let old_chars = old_text.chars().count();
        let new_chars = new_text.chars().count();
        AppliedEdit {
            start,
            old_text,
            new_text,
            old_chars,
            new_chars,
        }
    }
}

#[derive(Debug, Clone)]
struct Transaction {
    id: u64,
    edits: Vec<AppliedEdit>,
    before: Option<SelectionSet>,
    after: Option<SelectionSet>,
    mergeable: bool,
}

/// Linear undo/redo stacks. Each state of the text is identified by the id
/// of the last applied transaction (0 = initial), used for dirty tracking.
#[derive(Debug, Clone, Default)]
pub(crate) struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    merge_open: bool,
    next_id: u64,
}

impl History {
    pub(crate) fn current_state(&self) -> u64 {
        self.undo.last().map_or(0, |t| t.id)
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn break_merge(&mut self) {
        self.merge_open = false;
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn record(
        &mut self,
        edits: Vec<AppliedEdit>,
        group: UndoGroup,
        selections: Option<(SelectionSet, SelectionSet)>,
    ) {
        self.redo.clear();
        let merge = group == UndoGroup::Merge;
        let id = self.fresh_id();
        let (before, after) = match selections {
            Some((b, a)) => (Some(b), Some(a)),
            None => (None, None),
        };
        if merge
            && self.merge_open
            && let Some(last) = self.undo.last_mut()
            && last.mergeable
            && continues(last.after.as_ref(), before.as_ref())
        {
            last.id = id;
            last.edits.extend(edits);
            last.after = after;
            return;
        }
        self.undo.push(Transaction {
            id,
            edits,
            before,
            after,
            mergeable: merge,
        });
        self.merge_open = merge;
    }
}

/// Typing continues a step only where the previous one left the cursors
/// (when both steps recorded selections).
fn continues(prev_after: Option<&SelectionSet>, before: Option<&SelectionSet>) -> bool {
    match (prev_after, before) {
        (Some(a), Some(b)) => a.as_slice() == b.as_slice(),
        _ => true,
    }
}

/// Cursors at the end of each sequentially applied edit, mapped to the
/// final text.
///
/// Each edit moves earlier ends at or after its old end by its length
/// change and collapses ends inside it to its new end. Edits produced by
/// this crate run strictly back to front or front to back, which is O(1)
/// per edit (a shared offset); other orders fall back to updating every
/// end.
fn cursors_after(edits: &[Edit]) -> SelectionSet {
    // Actual end = stored + offset.
    let mut stored: Vec<isize> = Vec::with_capacity(edits.len());
    let mut offset: isize = 0;
    let (mut min, mut max) = (isize::MAX, isize::MIN);
    for e in edits {
        let (start, end) = (e.range.start as isize, e.range.end as isize);
        let new_end = e.new_end_char() as isize;
        let delta = new_end - end;
        if stored.is_empty() || (max < end && max <= start) {
            // Every earlier end lies before this edit: unaffected.
        } else if min >= end {
            // Every earlier end lies at or after this edit: all shift.
            offset += delta;
            min += delta;
            max += delta;
        } else {
            for p in &mut stored {
                let actual = *p + offset;
                let moved = if actual >= end {
                    actual + delta
                } else if actual > start {
                    new_end
                } else {
                    actual
                };
                *p = moved - offset;
            }
            min = stored
                .iter()
                .map(|p| p + offset)
                .min()
                .unwrap_or(isize::MAX);
            max = stored
                .iter()
                .map(|p| p + offset)
                .max()
                .unwrap_or(isize::MIN);
        }
        stored.push(new_end - offset);
        min = min.min(new_end);
        max = max.max(new_end);
    }
    let selections: Vec<Selection> = stored
        .into_iter()
        .map(|p| Selection::cursor((p + offset).max(0) as usize))
        .collect();
    SelectionSet::from_vec(selections, 0).unwrap_or_default()
}

impl Buffer {
    /// Reverts the last undo step. Returns `None` if there is nothing to undo.
    pub fn undo(&mut self) -> Option<UndoResult> {
        let tx = self.history.undo.pop()?;
        self.history.break_merge();
        let ops: Vec<(usize, usize, &str)> = tx
            .edits
            .iter()
            .rev()
            .map(|e| (e.start, e.new_chars, e.old_text.as_str()))
            .collect();
        let (edits, _) = self.apply_sequential(&ops);
        self.version += 1;
        let mut selections = tx.before.clone().unwrap_or_else(|| cursors_after(&edits));
        selections.clamp(self.len_chars());
        self.history.redo.push(tx);
        Some(UndoResult { edits, selections })
    }

    /// Re-applies the last undone step. Returns `None` if there is nothing
    /// to redo.
    pub fn redo(&mut self) -> Option<UndoResult> {
        let tx = self.history.redo.pop()?;
        self.history.break_merge();
        let ops: Vec<(usize, usize, &str)> = tx
            .edits
            .iter()
            .map(|e| (e.start, e.old_chars, e.new_text.as_str()))
            .collect();
        let (edits, _) = self.apply_sequential(&ops);
        self.version += 1;
        let mut selections = tx.after.clone().unwrap_or_else(|| cursors_after(&edits));
        selections.clamp(self.len_chars());
        self.history.undo.push(tx);
        Some(UndoResult { edits, selections })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The straightforward O(k^2) reference.
    fn naive(edits: &[Edit]) -> Vec<usize> {
        let mut ends: Vec<usize> = Vec::new();
        for e in edits {
            let old_len = e.range.len();
            let new_len = e.new_end_char() - e.range.start;
            for p in ends.iter_mut() {
                if *p >= e.range.end {
                    *p = *p + new_len - old_len;
                } else if *p > e.range.start {
                    *p = e.range.start + new_len;
                }
            }
            ends.push(e.new_end_char());
        }
        let mut sels: Vec<Selection> = ends.into_iter().map(Selection::cursor).collect();
        sels.sort_by_key(Selection::start);
        sels.dedup();
        sels.into_iter().map(|s| s.head).collect()
    }

    fn heads(set: &SelectionSet) -> Vec<usize> {
        set.iter().map(|s| s.head).collect()
    }

    #[test]
    fn cursors_after_matches_naive() {
        let cases: [&[(std::ops::Range<usize>, &str)]; 4] = [
            // Back to front (as applied by Buffer::edit).
            &[(8..9, "xyz"), (5..5, "ab"), (1..3, "")],
            // Front to back (as replayed by undo).
            &[(1..1, "ab"), (4..6, "q"), (9..9, "\n\n")],
            // Mixed order.
            &[(5..5, "abc"), (1..2, ""), (7..7, "z"), (0..9, "w")],
            &[(3..3, "a"), (3..3, "b"), (3..4, "")],
        ];
        for ops in cases {
            let mut b = Buffer::new("0123456789abcdef");
            let mut edits = Vec::new();
            for (r, t) in ops {
                edits.extend(b.edit(vec![(r.clone(), t.to_string())]));
            }
            assert_eq!(heads(&cursors_after(&edits)), naive(&edits), "{ops:?}");
        }
    }

    #[test]
    fn cursors_after_many_edits_is_fast() {
        let n = 50_000;
        let mut b = Buffer::new(&"ab\n".repeat(n));
        let edits = b.edit((0..n).map(|i| (i * 3..i * 3, "x".to_string())).collect());
        let start = std::time::Instant::now();
        let set = cursors_after(&edits);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(set.len(), n);
        assert_eq!(set.iter().next().map(|s| s.head), Some(1));
    }
}
