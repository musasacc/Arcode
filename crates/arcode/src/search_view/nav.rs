//! Moving through and opening Search view results with the keyboard or
//! the mouse.

use arcode_buffer::Point;
use gpui::{Context, ScrollStrategy, Window};

use super::results::Row;
use super::{ProjectSearch, ProjectSearchEvent};
use crate::finder::{Confirm, SelectNext, SelectPrev};

impl ProjectSearch {
    /// Activates row `ix`: a file header collapses or expands, a line opens
    /// its first match.
    pub fn open_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(&row) = self.rows.get(ix) else {
            return;
        };
        self.selected = Some(ix);
        match row {
            Row::File(fx) => {
                self.results.toggle(fx);
                self.rebuild_rows();
            }
            Row::Line(..) => {
                if let Some((file, line)) = self.results.line(row) {
                    let cols = line.columns.first().cloned().unwrap_or(0..0);
                    cx.emit(ProjectSearchEvent::Open {
                        path: file.path.clone(),
                        range: Point::new(line.line, cols.start)..Point::new(line.line, cols.end),
                    });
                }
            }
        }
        cx.notify();
    }

    pub(super) fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let ix = ix.min(self.rows.len() - 1);
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        cx.notify();
    }

    pub(super) fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected.map_or(0, |s| s.saturating_sub(1));
        self.select(ix, cx);
    }

    pub(super) fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected.map_or(0, |s| s + 1);
        self.select(ix, cx);
    }

    /// Enter: search now if the query changed, else open the selected
    /// (or first) match.
    pub(super) fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        if self.debounce.is_some() || self.last_query.as_ref() != Some(&self.build_query(cx)) {
            self.run(false, cx);
            return;
        }
        let ix = self
            .selected
            .filter(|&s| matches!(self.rows.get(s), Some(Row::Line(..))))
            .or_else(|| self.rows.iter().position(|r| matches!(r, Row::Line(..))));
        if let Some(ix) = ix {
            self.open_row(ix, cx);
        }
    }
}
