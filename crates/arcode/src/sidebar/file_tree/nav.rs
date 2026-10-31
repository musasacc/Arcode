//! Keyboard navigation: up / down / home / end, right expands (or steps
//! into an open folder), left collapses (or selects the parent), Enter or
//! Space opens.

use gpui::{Context, ScrollStrategy, Window};

use super::*;

impl FileTree {
    fn select_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(ix).filter(|r| !r.placeholder) {
            self.selected = Some(row.path.clone());
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
            cx.notify();
        }
    }

    pub(super) fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected_index().map_or(0, |i| i.saturating_sub(1));
        self.select_index(ix, cx);
    }

    pub(super) fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected_index().map_or(0, |i| i + 1);
        self.select_index(ix.min(self.rows.len().saturating_sub(1)), cx);
    }

    pub(super) fn select_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.select_index(0, cx);
    }

    pub(super) fn select_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.select_index(self.rows.len().saturating_sub(1), cx);
    }

    pub(super) fn expand(&mut self, _: &Expand, _: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_index() else {
            return;
        };
        let row = &self.rows[ix];
        if row.is_dir && !row.expanded {
            self.click(ix, cx);
        } else if row.is_dir && self.rows.get(ix + 1).is_some_and(|r| r.depth > row.depth) {
            // Already open: step into the first child.
            self.select_index(ix + 1, cx);
        }
    }

    pub(super) fn collapse(&mut self, _: &Collapse, _: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_index() else {
            return;
        };
        let row = &self.rows[ix];
        if row.is_dir && row.expanded {
            self.click(ix, cx);
        } else if let Some(parent) = row.path.parent().filter(|p| *p != self.root) {
            // Select the parent folder (it stays open).
            let parent = parent.to_path_buf();
            if let Some(pix) = self.rows.iter().position(|r| r.path == parent) {
                self.select_index(pix, cx);
            }
        }
    }

    pub(super) fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if let Some(ix) = self.selected_index() {
            self.click(ix, cx);
        }
    }
}
