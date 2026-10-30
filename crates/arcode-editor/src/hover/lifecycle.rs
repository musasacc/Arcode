//! When a hover closes or follows its symbol: cursor moves, diagnostics
//! changes, layout passes (scrolling) and the window's focus moving away.

use gpui::{App, Pixels, Window};

use crate::Editor;

impl Editor {
    /// Keyboard hovers (shown or still asked for) close when the cursor
    /// moves.
    pub(crate) fn hover_after_cursor_move(&mut self) {
        if self.hover.as_ref().is_some_and(|h| h.keyboard) {
            self.hover = None;
            self.cancel_hover_request();
        } else if self.hovering.pending_keyboard {
            self.cancel_hover_request();
        }
    }

    /// Called on render: when the window's focus moved and this editor does
    /// not have it, the hover closes (it is drawn above everything, so it
    /// would cover a picker or the finder that took the focus).
    pub(crate) fn hover_check_focus(&mut self, window: &Window, cx: &App) {
        let now = window.focused(cx).map(|f| f.downgrade());
        if now == self.hovering.last_focus {
            return;
        }
        self.hovering.last_focus = now;
        if !self.focus_handle.is_focused(window) {
            self.hide_hover();
        }
    }

    /// Called after each layout: a mouse hover closes when the view
    /// scrolled (the mouse is no longer over its symbol), any hover when
    /// its line left the view.
    pub(crate) fn hover_after_layout(&mut self, previous: Option<(Pixels, Pixels)>) {
        let Some(hover) = &self.hover else {
            return;
        };
        let Some(layout) = &self.layout else {
            return;
        };
        let scrolled = previous.is_some_and(|p| p != (layout.scroll_x, layout.scroll_y));
        if (scrolled && !hover.keyboard) || layout.visible(hover.anchor.line).is_none() {
            self.hover = None;
            self.cancel_hover_request();
        }
    }

    /// Diagnostics changed: refresh those in the hover, keep its docs.
    pub(crate) fn hover_after_diagnostics(&mut self) {
        let Some(mut hover) = self.hover.take() else {
            return;
        };
        hover.diagnostics = self.diagnostics_at(hover.at);
        if !hover.diagnostics.is_empty() || !hover.docs.is_empty() {
            self.hover = Some(hover);
        }
    }
}
