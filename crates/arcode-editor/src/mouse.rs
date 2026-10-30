//! Mouse handling: clicks, drag selection, multi-cursor clicks, wheel
//! scrolling and the scrollbar (hover lives in [`crate::hover`]).

use std::ops::Range;

use arcode_buffer::{Selection, SelectionSet, ops};
use gpui::{
    Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
    Window, point,
};

use crate::Editor;
use crate::editor::Autoscroll;
use crate::layout::LayoutSnapshot;

/// Granularity of a drag selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragMode {
    Char,
    Word,
    Line,
}

/// An in-progress mouse drag.
#[derive(Debug, Clone)]
pub(crate) enum DragState {
    /// Selecting text; `origin` is the unit first clicked.
    Text {
        mode: DragMode,
        origin: Range<usize>,
    },
    /// Dragging the scrollbar thumb, grabbed `grab` below its top.
    Scrollbar { grab: Pixels },
}

impl Editor {
    /// The selection unit (`mode`) around char `pos`.
    fn unit_at(&self, pos: usize, mode: DragMode) -> Range<usize> {
        match mode {
            DragMode::Char => pos..pos,
            DragMode::Word => ops::word_range_at(&self.buffer, pos).unwrap_or_else(|| {
                match self.buffer.char_at(pos) {
                    Some(c) if c != '\n' && !c.is_whitespace() => pos..pos + 1,
                    _ => pos..pos,
                }
            }),
            DragMode::Line => {
                let line = self.buffer.char_to_line(pos);
                self.buffer.line_start(line)..self.buffer.line_start(line + 1)
            }
        }
    }

    fn replace_primary(&mut self, sel: Selection) {
        let idx = self.selections.primary_index();
        let mut sels: Vec<Selection> = self.selections.iter().copied().collect();
        if let Some(slot) = sels.get_mut(idx) {
            *slot = sel;
        }
        if let Some(set) = SelectionSet::from_vec(sels, idx) {
            self.selections = set;
        }
    }

    pub(crate) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let Some(layout) = self.layout.clone() else {
            return;
        };
        if event.button != MouseButton::Left {
            return;
        }
        if self.mouse_down_ext(event, &layout, cx) || self.link_click(event, &layout, window, cx) {
            return;
        }
        let p = event.position;
        if let Some(bar) = layout.scrollbar
            && bar.track.contains(&p)
        {
            let grab = if bar.thumb.contains(&p) {
                p.y - bar.thumb.top()
            } else {
                bar.thumb.size.height * 0.5
            };
            self.drag = Some(DragState::Scrollbar { grab });
            self.drag_scrollbar(&layout, p.y, grab, cx);
            return;
        }
        let pos = self.char_for_position(&layout, p);
        let mode = if p.x < layout.text_bounds.left() {
            DragMode::Line
        } else {
            match event.click_count {
                0 | 1 => DragMode::Char,
                2 => DragMode::Word,
                _ => DragMode::Line,
            }
        };
        let unit = self.unit_at(pos, mode);
        self.buffer.break_undo_group();
        if event.modifiers.shift && mode == DragMode::Char {
            let anchor = self.selections.primary().anchor;
            self.replace_primary(Selection::new(anchor, pos));
            self.drag = Some(DragState::Text {
                mode,
                origin: anchor..anchor,
            });
        } else {
            let sel = Selection::new(unit.start, unit.end);
            if event.modifiers.alt {
                self.selections.add(sel);
            } else {
                self.selections = SelectionSet::from_selection(sel);
            }
            self.drag = Some(DragState::Text { mode, origin: unit });
        }
        self.autoscroll = Some(Autoscroll::Fit);
        self.selections_changed(cx);
    }

    pub(crate) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        hovered: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.layout.clone() else {
            return;
        };
        match self.drag.clone() {
            Some(DragState::Scrollbar { grab }) => {
                self.drag_scrollbar(&layout, event.position.y, grab, cx);
            }
            Some(DragState::Text { mode, origin })
                if event.pressed_button == Some(MouseButton::Left) =>
            {
                let pos = self.char_for_position(&layout, event.position);
                let unit = self.unit_at(pos, mode);
                let sel = if unit.start < origin.start {
                    Selection::new(origin.end, unit.start)
                } else {
                    Selection::new(origin.start, unit.end.max(origin.end))
                };
                if sel != *self.selections.primary() {
                    self.replace_primary(sel);
                    self.autoscroll = Some(Autoscroll::Fit);
                    self.selections_changed(cx);
                }
            }
            _ => self.hover_mouse_move(&layout, event.position, event.modifiers, hovered, cx),
        }
    }

    pub(crate) fn mouse_up(&mut self, _event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.clone() else {
            return;
        };
        let mut delta = event.delta.pixel_delta(layout.line_height);
        if event.modifiers.shift && delta.x == Pixels::ZERO {
            delta = point(delta.y, Pixels::ZERO);
        }
        let y = (self.scroll_y - delta.y).clamp(Pixels::ZERO, layout.max_scroll_y);
        let x = (self.scroll_x - delta.x).max(Pixels::ZERO);
        let hid = self.hide_hover();
        if y != self.scroll_y || x != self.scroll_x || hid {
            self.scroll_y = y;
            self.scroll_x = x;
            cx.notify();
        }
    }

    fn drag_scrollbar(
        &mut self,
        layout: &LayoutSnapshot,
        y: Pixels,
        grab: Pixels,
        cx: &mut Context<Self>,
    ) {
        let Some(bar) = layout.scrollbar else {
            return;
        };
        let range = bar.track.size.height - bar.thumb.size.height;
        if range <= Pixels::ZERO {
            return;
        }
        let frac = ((y - grab - bar.track.top()) / range).clamp(0., 1.);
        self.scroll_y = layout.max_scroll_y * frac;
        cx.notify();
    }
}
