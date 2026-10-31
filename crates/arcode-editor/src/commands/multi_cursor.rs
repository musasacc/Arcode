//! Multi-cursor commands beyond the basic ones: select all occurrences,
//! column (box) selection, Alt+click toggling and cursor undo.

use std::ops::Range;

use arcode_buffer::{Buffer, Selection, SelectionSet, movement, ops, search};
use gpui::{Context, Pixels, Window};

use crate::Editor;
use crate::actions::{SelectAllOccurrences, UndoCursor};

/// How many earlier selection states [`UndoCursor`] can go back to.
const HISTORY: usize = 64;

/// Earlier selections for [`UndoCursor`] (cleared by edits).
#[derive(Default)]
pub(crate) struct CursorHistory {
    /// The selections after the last recorded change.
    current: Option<SelectionSet>,
    /// Earlier states, oldest first.
    stack: Vec<SelectionSet>,
}

impl CursorHistory {
    /// Records that the selections are now `now`.
    pub(crate) fn record(&mut self, now: &SelectionSet) {
        if let Some(prev) = self.current.replace(now.clone())
            && prev != *now
        {
            self.stack.push(prev);
            if self.stack.len() > HISTORY {
                self.stack.remove(0);
            }
        }
    }

    /// After an edit old positions mean nothing: start over from `now`.
    pub(crate) fn reset(&mut self, now: &SelectionSet) {
        self.stack.clear();
        self.current = Some(now.clone());
    }

    /// The state before the current one.
    fn pop(&mut self) -> Option<SelectionSet> {
        let prev = self.stack.pop()?;
        self.current = Some(prev.clone());
        Some(prev)
    }
}

/// Every occurrence of the primary selection's text (case-sensitive), or
/// of the word under an empty primary cursor (whole words only). The
/// occurrence at the primary stays primary. `None` if there is nothing to
/// search for.
pub(crate) fn all_occurrences(buffer: &Buffer, selections: &SelectionSet) -> Option<SelectionSet> {
    let primary = *selections.primary();
    let (range, whole_word) = if primary.is_empty() {
        (ops::word_range_at(buffer, primary.head)?, true)
    } else {
        (primary.range(), false)
    };
    let query = buffer.slice_to_string(range.clone());
    let is_word = |p: Option<char>| p.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let found: Vec<Range<usize>> = search::find_all(buffer, &query, true)
        .into_iter()
        .filter(|r| {
            !whole_word
                || (!is_word(r.start.checked_sub(1).and_then(|p| buffer.char_at(p)))
                    && !is_word(buffer.char_at(r.end)))
        })
        .collect();
    let index = found.iter().position(|r| *r == range).unwrap_or(0);
    let sels = found
        .into_iter()
        .map(|r| Selection::new(r.start, r.end))
        .collect();
    SelectionSet::from_vec(sels, index)
}

/// Where an x offset falls on one line: the char column the caret goes to
/// and the x where the line's text ends.
pub(crate) type ColumnHit<'a> = dyn Fn(usize, Pixels) -> (usize, Pixels) + 'a;

/// A [`ColumnHit`] on a grid of `char_width` cells (tabs count by their
/// width): for lines that are not laid out.
pub(crate) fn grid_hit(
    buffer: &Buffer,
    tab_width: usize,
    char_width: Pixels,
) -> impl Fn(usize, Pixels) -> (usize, Pixels) + '_ {
    move |line, x| {
        let col = (x / char_width).round().max(0.) as usize;
        let start = buffer.line_start(line);
        let pos = movement::char_for_display_column(buffer, line, col, tab_width);
        let end = movement::display_column(buffer, start + buffer.line_len(line), tab_width);
        (pos - start, char_width * end as f32)
    }
}

/// A box selection from `anchor` to `head`, both `(line, x)` with x from
/// the text origin: one selection per line, from the char under the
/// anchor's x to the one under the head's (`hit` maps x to a char per
/// line, so wide glyphs and tabs line up with what is drawn). Lines whose
/// text ends left of the box are skipped, unless the box has no width
/// (less than half a `char_width`: then every line gets a cursor, at its
/// end if it is short). The selection on the head's line is primary.
pub(crate) fn column_selections(
    buffer: &Buffer,
    anchor: (usize, Pixels),
    head: (usize, Pixels),
    char_width: Pixels,
    hit: &ColumnHit,
) -> SelectionSet {
    let (first, last) = (anchor.0.min(head.0), anchor.0.max(head.0));
    let left = anchor.1.min(head.1);
    let zero_width = (anchor.1 - head.1).abs() < char_width / 2.;
    let mut sels = Vec::new();
    let mut primary = 0;
    for line in first..=last {
        let start = buffer.line_start(line);
        let (a, end_x) = hit(line, anchor.1);
        if end_x + char_width / 2. <= left && !zero_width {
            continue;
        }
        let h = if zero_width { a } else { hit(line, head.1).0 };
        if line == head.0 {
            primary = sels.len();
        }
        sels.push(Selection::new(start + a, start + h));
    }
    if sels.is_empty() {
        let pos = buffer.line_start(head.0) + hit(head.0, head.1).0;
        return SelectionSet::single(pos);
    }
    SelectionSet::from_vec(sels, primary).unwrap_or_default()
}

/// `selections` without the one containing `pos` (a cursor at `pos` or a
/// selection around it). `None` if none contains it or it is the only one.
pub(crate) fn without_selection_at(selections: &SelectionSet, pos: usize) -> Option<SelectionSet> {
    if selections.len() < 2 {
        return None;
    }
    let hit = selections.iter().position(|s| {
        if s.is_empty() {
            s.head == pos
        } else {
            s.start() <= pos && pos <= s.end()
        }
    })?;
    let primary = selections.primary_index();
    let rest: Vec<Selection> = selections
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != hit)
        .map(|(_, s)| *s)
        .collect();
    let primary = match primary {
        p if p == hit => rest.len() - 1,
        p if p > hit => p - 1,
        p => p,
    };
    SelectionSet::from_vec(rest, primary)
}

impl Editor {
    pub(crate) fn select_all_occurrences(
        &mut self,
        _: &SelectAllOccurrences,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selections(cx, |b, s| {
            if let Some(all) = all_occurrences(b, s) {
                *s = all;
            }
        });
    }

    pub(crate) fn undo_cursor(&mut self, _: &UndoCursor, _: &mut Window, cx: &mut Context<Self>) {
        let Some(mut prev) = self.cursor_history.pop() else {
            return;
        };
        prev.clamp(self.buffer.len_chars());
        self.selections = prev;
        self.buffer.break_undo_group();
        self.selections_changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    fn ranges(s: &SelectionSet) -> Vec<(usize, usize)> {
        s.iter().map(|s| (s.anchor, s.head)).collect()
    }

    #[test]
    fn occurrences_of_the_word_or_the_selection() {
        let b = Buffer::new("foo food foo\nfoo");
        let all = all_occurrences(&b, &SelectionSet::single(9)).unwrap();
        assert_eq!(ranges(&all), vec![(0, 3), (9, 12), (13, 16)]);
        assert_eq!(all.primary().range(), 9..12);
        let sel = SelectionSet::from_selection(Selection::new(0, 3));
        let all = all_occurrences(&b, &sel).unwrap();
        assert_eq!(all.len(), 4, "a selection also matches inside words");
        assert!(all_occurrences(&b, &SelectionSet::single(3)).is_some());
        assert!(all_occurrences(&Buffer::new("  "), &SelectionSet::single(1)).is_none());
    }

    #[test]
    fn column_selection_skips_short_lines() {
        let b = Buffer::new("abcdef\nab\nabcdef\n\tx");
        let cw = px(10.);
        let hit = grid_hit(&b, 4, cw);
        let col = |line: usize, c: f32| (line, cw * c);
        let s = column_selections(&b, col(0, 1.), col(2, 4.), cw, &hit);
        assert_eq!(ranges(&s), vec![(1, 4), (8, 9), (11, 14)]);
        assert_eq!(s.primary_index(), 2);
        // Line 1 ends left of this box.
        let s = column_selections(&b, col(0, 3.), col(2, 4.), cw, &hit);
        assert_eq!(ranges(&s), vec![(3, 4), (13, 14)]);
        // Dragging leftwards puts the heads on the left.
        let s = column_selections(&b, col(0, 4.), col(0, 1.), cw, &hit);
        assert_eq!(ranges(&s), vec![(4, 1)]);
        // A zero-width box puts a cursor on every line, clamped.
        let s = column_selections(&b, col(0, 5.), col(3, 5.2), cw, &hit);
        assert_eq!(ranges(&s), vec![(5, 5), (9, 9), (15, 15), (19, 19)]);
        // A tab counts as its display width.
        let s = column_selections(&b, col(3, 4.), col(3, 5.), cw, &hit);
        assert_eq!(ranges(&s), vec![(18, 19)]);
    }

    #[test]
    fn column_selection_follows_wide_glyphs() {
        // Line 1 is drawn with glyphs twice as wide as the char width.
        let b = Buffer::new("abcdef\n漢字漢字漢字\nabcdef");
        let cw = px(10.);
        let grid = grid_hit(&b, 4, cw);
        let hit = |line: usize, x: Pixels| {
            if line != 1 {
                return grid(line, x);
            }
            let col = ((x / (cw * 2.)).round() as usize).min(6);
            (col, cw * 12.)
        };
        let s = column_selections(&b, (0, cw), (2, cw * 5.), cw, &hit);
        assert_eq!(ranges(&s), vec![(1, 5), (8, 10), (15, 19)]);
    }

    #[test]
    fn alt_click_removes_a_cursor() {
        let set = SelectionSet::from_vec(
            vec![
                Selection::cursor(1),
                Selection::new(4, 6),
                Selection::cursor(9),
            ],
            2,
        )
        .unwrap();
        let rest = without_selection_at(&set, 5).unwrap();
        assert_eq!(ranges(&rest), vec![(1, 1), (9, 9)]);
        assert_eq!(rest.primary().head, 9);
        assert!(without_selection_at(&set, 3).is_none());
        assert!(without_selection_at(&SelectionSet::single(1), 1).is_none());
    }

    #[test]
    fn history_steps_back() {
        let mut h = CursorHistory::default();
        h.record(&SelectionSet::single(1));
        h.record(&SelectionSet::single(2));
        h.record(&SelectionSet::single(2));
        h.record(&SelectionSet::single(3));
        assert_eq!(h.pop(), Some(SelectionSet::single(2)));
        assert_eq!(h.pop(), Some(SelectionSet::single(1)));
        assert_eq!(h.pop(), None);
        h.reset(&SelectionSet::single(5));
        assert_eq!(h.pop(), None);
    }
}
