//! Mouse side of hover: resting on a symbol, the definition link while the
//! secondary modifier is held, and secondary-click to go to definition.

use std::ops::Range;
use std::time::Duration;

use arcode_buffer::{Point, SelectionSet};
use gpui::{Bounds, Context, Modifiers, MouseDownEvent, Pixels, Window};

use super::HoverState;
use crate::Editor;
use crate::decorate::range_rects;
use crate::ext::RowKind;
use crate::layout::LayoutSnapshot;

impl Editor {
    /// The buffer point under window position `p`: a char of a laid-out
    /// line, or the one-char slot after its end.
    pub(crate) fn mouse_point(
        &self,
        layout: &LayoutSnapshot,
        p: gpui::Point<Pixels>,
    ) -> Option<Point> {
        if !layout.text_bounds.contains(&p) {
            return None;
        }
        let RowKind::Text(line) = layout.row_at(p.y, self.buffer.line_count()) else {
            return None;
        };
        if p.y >= layout.line_top(line) + layout.line_height {
            return None;
        }
        let visible = layout.visible(line)?;
        let x = p.x - layout.text_left();
        let line_len = self.buffer.line_len(line);
        let col = if x >= visible.shaped.width {
            if x > visible.shaped.width + layout.char_width {
                return None;
            }
            line_len
        } else {
            let byte = visible.shaped.index_for_x(x)?;
            visible.display.col_at_byte(byte).min(line_len)
        };
        Some(Point::new(line, col))
    }

    /// Whether `p` is inside the drawn hover popover.
    fn over_hover_popover(&self, p: gpui::Point<Pixels>) -> bool {
        self.hover.is_some()
            && self
                .hovering
                .popover_bounds
                .get()
                .is_some_and(|b| b.contains(&p))
    }

    /// Mouse moved without a drag: lens hover, definition link, hover.
    pub(crate) fn hover_mouse_move(
        &mut self,
        layout: &LayoutSnapshot,
        p: gpui::Point<Pixels>,
        modifiers: Modifiers,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.hover_lens(layout, p);
        if !self.over_hover_popover(p) {
            let at = if hovered {
                self.mouse_point(layout, p)
            } else {
                None
            };
            changed |= self.update_link(at, modifiers, cx);
            changed |= self.update_mouse_hover(at, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Modifiers changed: show or hide the link under the resting mouse.
    pub(crate) fn hover_modifiers_changed(
        &mut self,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.layout.clone() else {
            return;
        };
        let p = window.mouse_position();
        let at = if self.over_hover_popover(p) {
            None
        } else {
            self.mouse_point(&layout, p)
        };
        if self.update_link(at, modifiers, cx) {
            cx.notify();
        }
    }

    /// Updates the popover for the mouse at `at`. Returns whether it changed.
    fn update_mouse_hover(&mut self, at: Option<Point>, cx: &mut Context<Self>) -> bool {
        if let (Some(h), Some(p)) = (&self.hover, at)
            && !h.docs.is_empty()
            && h.range.as_ref().is_some_and(|r| r.start <= p && p < r.end)
        {
            return false;
        }
        let diagnostics = at.map(|p| self.diagnostics_at(p)).unwrap_or_default();
        let keyboard_shown = self.hover.as_ref().is_some_and(|h| h.keyboard);
        // `None`: keep what is shown.
        let next = match at {
            Some(p) if !diagnostics.is_empty() => {
                Some(self.hover_state(p, diagnostics, Vec::new(), None, false))
            }
            _ if keyboard_shown => None,
            _ => Some(None),
        };
        // A keyboard request in flight is not the mouse's to cancel.
        if !self.hovering.pending_keyboard {
            let word = at.and_then(|p| Some((self.word_under(p)?, p)));
            let enabled = arcode_ui_kit::settings(cx).editor.hover;
            match word {
                Some((word, p)) if enabled => {
                    if self.hovering.pending.as_ref() != Some(&word) {
                        let delay = arcode_ui_kit::settings(cx).editor.hover_delay_ms;
                        self.request_hover(word, p, Duration::from_millis(delay), false, cx);
                    }
                }
                _ => {
                    if self.hovering.pending.is_some() {
                        self.cancel_hover_request();
                    }
                }
            }
        }
        match next {
            Some(next) if !same_popover(next.as_ref(), self.hover.as_ref()) => {
                self.hover = next;
                true
            }
            _ => false,
        }
    }

    /// Updates the definition link for the mouse at `at`. Returns whether
    /// the underline changed.
    fn update_link(
        &mut self,
        at: Option<Point>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let link_keys = modifiers.secondary() && !modifiers.alt && !modifiers.shift;
        let word = if link_keys && self.hovering.provider.is_some() && self.drag.is_none() {
            at.and_then(|p| Some((self.word_under(p)?, p)))
        } else {
            None
        };
        let Some((word, p)) = word else {
            self.hovering.link_word = None;
            self.hovering.link_task = None;
            return self.hovering.link.take().is_some();
        };
        if self.hovering.link_word.as_ref() == Some(&word) {
            return false;
        }
        self.hovering.link_word = Some(word.clone());
        let had_link = self.hovering.link.take().is_some();
        let Some(provider) = self.hovering.provider.clone() else {
            return had_link;
        };
        self.hovering.link_task = Some(cx.spawn(async move |this, cx| {
            let Ok(Some(request)) = this.update(cx, |e, cx| provider.definition_link(e, p, cx))
            else {
                return;
            };
            let answer = request.await;
            this.update(cx, |e, cx| e.link_arrived(word, answer, cx))
                .ok();
        }));
        had_link
    }

    fn link_arrived(
        &mut self,
        word: Range<usize>,
        answer: anyhow::Result<Option<Range<Point>>>,
        cx: &mut Context<Self>,
    ) {
        if self.hovering.link_word.as_ref() != Some(&word) {
            return;
        }
        let link = answer.ok().flatten();
        if link != self.hovering.link {
            self.hovering.link = link;
            cx.notify();
        }
    }

    /// Secondary-click on a symbol: moves the cursor there and dispatches
    /// [`crate::GoToDefinition`]. Returns whether the click was consumed.
    pub(crate) fn link_click(
        &mut self,
        event: &MouseDownEvent,
        layout: &LayoutSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let m = event.modifiers;
        if !m.secondary() || m.alt || m.shift || self.hovering.provider.is_none() {
            return false;
        }
        let Some(at) = self.mouse_point(layout, event.position) else {
            return false;
        };
        if self.word_under(at).is_none() {
            return false;
        }
        let pos = self.buffer.point_to_char(at);
        self.selections = SelectionSet::single(pos);
        self.buffer.break_undo_group();
        self.hide_hover();
        self.selections_changed(cx);
        window.dispatch_action(Box::new(crate::GoToDefinition), cx);
        true
    }

    /// Window rectangles of the definition link (pointer cursor).
    pub(crate) fn link_bounds(&self, layout: &LayoutSnapshot) -> Vec<Bounds<Pixels>> {
        match &self.hovering.link {
            Some(link) => range_rects(self, layout, link.start, link.end, false),
            None => Vec::new(),
        }
    }
}

/// Whether two popovers look the same (the point they were computed for
/// may differ): moving within one diagnostic redraws nothing.
fn same_popover(a: Option<&HoverState>, b: Option<&HoverState>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.anchor == b.anchor
                && a.keyboard == b.keyboard
                && a.range == b.range
                && a.diagnostics == b.diagnostics
                && a.docs == b.docs
        }
        _ => false,
    }
}
