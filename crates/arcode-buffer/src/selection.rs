//! Selections and multi-cursor selection sets.

use std::ops::Range;

/// A selection: `anchor` is where it started, `head` is the cursor.
///
/// Positions are char indices. An empty selection (`anchor == head`) is a
/// plain cursor. `goal_column` is the display column vertical movement
/// tries to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Selection {
    /// The fixed end.
    pub anchor: usize,
    /// The moving end (the cursor).
    pub head: usize,
    /// Display column remembered across vertical moves.
    pub goal_column: Option<usize>,
}

impl Selection {
    /// A selection from `anchor` to `head`.
    pub fn new(anchor: usize, head: usize) -> Self {
        Selection {
            anchor,
            head,
            goal_column: None,
        }
    }

    /// An empty selection (cursor) at `pos`.
    pub fn cursor(pos: usize) -> Self {
        Selection::new(pos, pos)
    }

    /// The selected range, ordered.
    pub fn range(&self) -> Range<usize> {
        self.start()..self.end()
    }

    /// The smaller end.
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// The larger end.
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// True if nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// True if the head is before the anchor.
    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    /// The cursor position (the head).
    pub fn head(&self) -> usize {
        self.head
    }

    /// Moves the head to `pos`; without `extend` the anchor follows.
    /// Clears the goal column.
    pub fn move_to(&mut self, pos: usize, extend: bool) {
        self.head = pos;
        if !extend {
            self.anchor = pos;
        }
        self.goal_column = None;
    }

    /// Returns the selection with both ends clamped to `max`.
    pub fn clamped(self, max: usize) -> Self {
        Selection {
            anchor: self.anchor.min(max),
            head: self.head.min(max),
            goal_column: self.goal_column,
        }
    }
}

/// A non-empty set of selections with a primary one.
///
/// After [`SelectionSet::normalize`] (done by every constructor and
/// mutator) selections are sorted and non-overlapping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SelectionSet {
    selections: Vec<Selection>,
    primary: usize,
}

impl Default for SelectionSet {
    fn default() -> Self {
        SelectionSet::single(0)
    }
}

impl SelectionSet {
    /// A single cursor at `pos`.
    pub fn single(pos: usize) -> Self {
        SelectionSet::from_selection(Selection::cursor(pos))
    }

    /// A set holding one selection.
    pub fn from_selection(selection: Selection) -> Self {
        SelectionSet {
            selections: vec![selection],
            primary: 0,
        }
    }

    /// Builds a normalized set; `None` if `selections` is empty. `primary`
    /// is clamped to a valid index.
    pub fn from_vec(selections: Vec<Selection>, primary: usize) -> Option<Self> {
        if selections.is_empty() {
            return None;
        }
        let primary = primary.min(selections.len() - 1);
        let mut set = SelectionSet {
            selections,
            primary,
        };
        set.normalize();
        Some(set)
    }

    /// Adds a selection, makes it primary and normalizes.
    pub fn add(&mut self, selection: Selection) {
        self.selections.push(selection);
        self.primary = self.selections.len() - 1;
        self.normalize();
    }

    /// The primary selection.
    pub fn primary(&self) -> &Selection {
        &self.selections[self.primary]
    }

    /// Index of the primary selection.
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Makes selection `index` primary (clamped).
    pub fn set_primary(&mut self, index: usize) {
        self.primary = index.min(self.selections.len() - 1);
    }

    /// Number of selections (at least one).
    pub fn len(&self) -> usize {
        self.selections.len()
    }

    /// Always false; a set holds at least one selection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Iterates selections in document order.
    pub fn iter(&self) -> std::slice::Iter<'_, Selection> {
        self.selections.iter()
    }

    /// The selections as a slice, in document order.
    pub fn as_slice(&self) -> &[Selection] {
        &self.selections
    }

    /// Keeps only the primary selection.
    pub fn collapse_to_primary(&mut self) {
        let primary = *self.primary();
        self.selections = vec![primary];
        self.primary = 0;
    }

    /// Replaces every selection with `f(selection)` and normalizes.
    pub fn transform(&mut self, mut f: impl FnMut(Selection) -> Selection) {
        for s in self.selections.iter_mut() {
            *s = f(*s);
        }
        self.normalize();
    }

    /// Clamps all positions to `0..=max` and normalizes.
    pub fn clamp(&mut self, max: usize) {
        self.transform(|s| s.clamped(max));
    }

    /// Sorts selections and merges overlapping ones, as well as cursors that
    /// touch another selection (equal cursors, or a cursor at the edge of a
    /// range). Two non-empty selections that merely touch stay separate so
    /// adjacent occurrences (e.g. from "select next occurrence") survive.
    /// The primary follows its selection into a merge.
    pub fn normalize(&mut self) {
        if self.selections.len() < 2 {
            return;
        }
        let mut indexed: Vec<(usize, Selection)> = std::mem::take(&mut self.selections)
            .into_iter()
            .enumerate()
            .collect();
        indexed.sort_by_key(|(_, s)| (s.start(), s.end()));
        let mut out: Vec<Selection> = Vec::with_capacity(indexed.len());
        let mut new_primary = 0;
        for (idx, sel) in indexed {
            if let Some(last) = out.last_mut() {
                let touching = sel.start() == last.end() && (sel.is_empty() || last.is_empty());
                if sel.start() < last.end() || touching {
                    let start = last.start();
                    let end = last.end().max(sel.end());
                    let reversed = last.is_reversed() || (last.is_empty() && sel.is_reversed());
                    *last = if reversed {
                        Selection::new(end, start)
                    } else {
                        Selection::new(start, end)
                    };
                    if idx == self.primary {
                        new_primary = out.len() - 1;
                    }
                    continue;
                }
            }
            if idx == self.primary {
                new_primary = out.len();
            }
            out.push(sel);
        }
        self.selections = out;
        self.primary = new_primary;
    }
}

impl<'a> IntoIterator for &'a SelectionSet {
    type Item = &'a Selection;
    type IntoIter = std::slice::Iter<'a, Selection>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_basics() {
        let s = Selection::new(5, 2);
        assert_eq!(s.range(), 2..5);
        assert!(s.is_reversed());
        assert!(!s.is_empty());
        assert_eq!(s.head(), 2);
        let mut c = Selection::cursor(3);
        assert!(c.is_empty());
        c.goal_column = Some(7);
        c.move_to(6, true);
        assert_eq!((c.anchor, c.head, c.goal_column), (3, 6, None));
        c.move_to(1, false);
        assert_eq!(c, Selection::cursor(1));
        assert_eq!(Selection::new(9, 20).clamped(10), Selection::new(9, 10));
    }

    #[test]
    fn normalize_sorts_and_merges() {
        let set = SelectionSet::from_vec(
            vec![
                Selection::new(10, 12),
                Selection::cursor(3),
                Selection::new(0, 4),  // overlaps cursor 3
                Selection::cursor(12), // touches 10..12
                Selection::cursor(20),
                Selection::cursor(20),
            ],
            4,
        )
        .expect("non-empty");
        let ranges: Vec<_> = set.iter().map(|s| s.range()).collect();
        assert_eq!(ranges, vec![0..4, 10..12, 20..20]);
        assert_eq!(set.primary_index(), 2);
        assert_eq!(set.primary().head, 20);
    }

    #[test]
    fn touching_ranges_stay_separate() {
        let set = SelectionSet::from_vec(vec![Selection::new(2, 4), Selection::new(0, 2)], 0)
            .expect("non-empty");
        assert_eq!(set.len(), 2);
        assert_eq!(set.primary().range(), 2..4);
    }

    #[test]
    fn merge_keeps_direction_and_primary() {
        let set = SelectionSet::from_vec(vec![Selection::new(5, 1), Selection::new(3, 8)], 1)
            .expect("non-empty");
        assert_eq!(set.len(), 1);
        assert_eq!(set.primary(), &Selection::new(8, 1));
    }

    #[test]
    fn add_and_primary() {
        let mut set = SelectionSet::single(5);
        set.add(Selection::cursor(1));
        assert_eq!(set.len(), 2);
        assert_eq!(set.primary().head, 1);
        assert_eq!(set.primary_index(), 0);
        set.set_primary(9);
        assert_eq!(set.primary().head, 5);
        set.collapse_to_primary();
        assert_eq!(set.len(), 1);
        assert_eq!(set.primary().head, 5);
        assert!(SelectionSet::from_vec(vec![], 0).is_none());
        assert!(!set.is_empty());
    }

    #[test]
    fn goal_column_survives_normalize() {
        let mut sel = Selection::cursor(4);
        sel.goal_column = Some(9);
        let set = SelectionSet::from_vec(vec![Selection::cursor(1), sel], 1).expect("set");
        assert_eq!(set.primary().goal_column, Some(9));
    }

    #[test]
    fn clamp_and_transform() {
        let mut set = SelectionSet::from_vec(vec![Selection::cursor(3), Selection::new(8, 15)], 0)
            .expect("set");
        set.clamp(5);
        let ranges: Vec<_> = set.as_slice().iter().map(|s| s.range()).collect();
        assert_eq!(ranges, vec![3..3, 5..5]);
        set.transform(|_| Selection::cursor(1));
        assert_eq!(set.len(), 1);
        let n = (&set).into_iter().count();
        assert_eq!(n, 1);
    }
}
