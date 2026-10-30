//! Cursor movement and selection actions.

use arcode_buffer::{Buffer, Selection, SelectionSet, movement, ops};
use gpui::{Context, Window};

use crate::Editor;
use crate::actions::*;

impl Editor {
    fn tab_width(&self, cx: &gpui::App) -> usize {
        self.ops_config(cx).tab_width
    }

    fn page_lines(&self) -> usize {
        self.layout
            .as_ref()
            .map_or(20, |l| l.page_lines().saturating_sub(1).max(1))
    }

    /// Applies a selection-only change and syncs state.
    pub(crate) fn update_selections(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&Buffer, &mut SelectionSet),
    ) {
        let before = self.selections.clone();
        f(&self.buffer, &mut self.selections);
        if before != self.selections {
            self.buffer.break_undo_group();
            self.selections_changed(cx);
        } else {
            // Still bring the cursor into view (e.g. after scrolling away).
            self.autoscroll = Some(crate::editor::Autoscroll::Fit);
            cx.notify();
        }
    }

    fn horizontal(&mut self, extend: bool, right: bool, cx: &mut Context<Self>) {
        self.update_selections(cx, |b, s| {
            if right {
                movement::move_right(b, s, extend)
            } else {
                movement::move_left(b, s, extend)
            }
        });
    }

    fn vertical(&mut self, extend: bool, delta: isize, cx: &mut Context<Self>) {
        let tw = self.tab_width(cx);
        let lines = delta.unsigned_abs();
        self.update_selections(cx, |b, s| {
            movement::move_selections_vertically(b, s, extend, |b, pos, goal| {
                if delta < 0 {
                    movement::page_up(b, pos, goal, lines, tw)
                } else {
                    movement::page_down(b, pos, goal, lines, tw)
                }
            })
        });
    }

    fn move_by(
        &mut self,
        extend: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&Buffer, usize) -> usize,
    ) {
        self.update_selections(cx, |b, s| movement::move_selections(b, s, extend, f));
    }

    pub(crate) fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(false, false, cx);
    }
    pub(crate) fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(false, true, cx);
    }
    pub(crate) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(true, false, cx);
    }
    pub(crate) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(true, true, cx);
    }
    pub(crate) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, -1, cx);
    }
    pub(crate) fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, 1, cx);
    }
    pub(crate) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, -1, cx);
    }
    pub(crate) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, 1, cx);
    }
    pub(crate) fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.page(false, -1, cx);
    }
    pub(crate) fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.page(false, 1, cx);
    }
    pub(crate) fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page(true, -1, cx);
    }
    pub(crate) fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page(true, 1, cx);
    }

    fn page(&mut self, extend: bool, dir: isize, cx: &mut Context<Self>) {
        let lines = self.page_lines();
        if let Some(layout) = self.layout.clone() {
            let delta = layout.line_height * (lines as f32 * dir as f32);
            self.scroll_y = (self.scroll_y + delta).clamp(gpui::Pixels::ZERO, layout.max_scroll_y);
        }
        self.vertical(extend, dir * lines as isize, cx);
    }

    pub(crate) fn move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(false, cx, movement::word_left);
    }
    pub(crate) fn move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(false, cx, movement::word_right);
    }
    pub(crate) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(true, cx, movement::word_left);
    }
    pub(crate) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(true, cx, movement::word_right);
    }
    pub(crate) fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_by(false, cx, movement::line_start_smart);
    }
    pub(crate) fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_by(false, cx, movement::line_end);
    }
    pub(crate) fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_by(true, cx, movement::line_start_smart);
    }
    pub(crate) fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_by(true, cx, movement::line_end);
    }
    pub(crate) fn move_doc_start(
        &mut self,
        _: &MoveDocStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selections(cx, |_, s| *s = SelectionSet::single(0));
    }
    pub(crate) fn move_doc_end(&mut self, _: &MoveDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.update_selections(cx, |b, s| *s = SelectionSet::single(b.len_chars()));
    }
    pub(crate) fn select_doc_start(
        &mut self,
        _: &SelectDocStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(true, cx, |_, _| 0);
    }
    pub(crate) fn select_doc_end(
        &mut self,
        _: &SelectDocEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_by(true, cx, |b, _| b.len_chars());
    }
    pub(crate) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.update_selections(cx, ops::select_all);
    }

    pub(crate) fn select_next_occurrence(
        &mut self,
        _: &SelectNextOccurrence,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selections(cx, |b, s| {
            ops::select_next_occurrence(b, s);
        });
    }

    pub(crate) fn add_cursor_above(
        &mut self,
        _: &AddCursorAbove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_cursor_vertically(-1, cx);
    }

    pub(crate) fn add_cursor_below(
        &mut self,
        _: &AddCursorBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_cursor_vertically(1, cx);
    }

    /// Adds a cursor one line above the topmost (or below the bottommost)
    /// selection, keeping the primary's goal column.
    fn add_cursor_vertically(&mut self, dir: isize, cx: &mut Context<Self>) {
        let tw = self.tab_width(cx);
        let goal = self.selections.primary().goal_column;
        self.update_selections(cx, |b, s| {
            let edge = if dir < 0 {
                s.iter().next()
            } else {
                s.iter().last()
            };
            let Some(edge) = edge.copied() else {
                return;
            };
            let goal = goal.or_else(|| Some(movement::display_column(b, edge.head, tw)));
            let line = b.char_to_line(edge.head);
            let target = line as isize + dir;
            if target < 0 || target as usize >= b.line_count() {
                return;
            }
            let (pos, goal) = if dir < 0 {
                movement::up(b, edge.head, goal, tw)
            } else {
                movement::down(b, edge.head, goal, tw)
            };
            let mut sel = Selection::cursor(pos);
            sel.goal_column = Some(goal);
            s.add(sel);
        });
    }

    pub(crate) fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.snippet_cancel() {
            cx.notify();
        } else if self.selections.len() > 1 {
            self.update_selections(cx, |_, s| s.collapse_to_primary());
        } else if !self.selections.primary().is_empty() {
            let head = self.selections.primary().head;
            self.update_selections(cx, |_, s| *s = SelectionSet::single(head));
        } else if self.hover.take().is_some() {
            cx.notify();
        } else {
            cx.propagate();
        }
    }
}
