//! Mouse handling: click to focus, drag to select, wheel to scroll.

use arcode_terminal::{GridPoint, KeyMods, SelectionKind, encode_key};
use gpui::{
    Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, Window,
};

use crate::input::{wheel_lines, wheel_report};
use crate::layout::grid_point;
use crate::view::{Drag, FrameInfo, TerminalView};

/// The grid cell under a window position in the last painted frame.
fn cell_at(frame: &FrameInfo, position: Point<Pixels>) -> GridPoint {
    let local = position - frame.origin;
    grid_point(
        local.x.into(),
        local.y.into(),
        frame.cell_width.into(),
        frame.line_height.into(),
        frame.cols,
        frame.rows,
    )
}

impl TerminalView {
    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let Some(frame) = self.frame else { return };
        let point = cell_at(&frame, event.position);
        if event.modifiers.shift && self.terminal.selection_text().is_some() {
            self.terminal.selection_update(point);
            self.drag = Some(Drag {
                anchor: point,
                started: true,
                last: point,
            });
            cx.notify();
            return;
        }
        let kind = match event.click_count {
            0 | 1 => None,
            2 => Some(SelectionKind::Semantic),
            _ => Some(SelectionKind::Lines),
        };
        self.terminal.clear_selection();
        if let Some(kind) = kind {
            self.terminal.selection_start_with(point, kind);
            self.terminal.selection_update(point);
        }
        self.drag = Some(Drag {
            anchor: point,
            started: kind.is_some(),
            last: point,
        });
        cx.notify();
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(frame), Some(mut drag)) = (self.frame, self.drag) else {
            return;
        };
        if !event.dragging() {
            return;
        }
        let point = cell_at(&frame, event.position);
        if drag.started && point == drag.last {
            return;
        }
        if !drag.started {
            if point == drag.anchor {
                return;
            }
            self.terminal.selection_start(drag.anchor);
            drag.started = true;
        }
        self.terminal.selection_update(point);
        drag.last = point;
        self.drag = Some(drag);
        cx.notify();
    }

    pub(crate) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    pub(crate) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.frame else { return };
        let line_height = frame.line_height;
        let delta = event.delta.pixel_delta(line_height).y;
        let lines = wheel_lines(&mut self.scroll_remainder, delta.into(), line_height.into());
        cx.stop_propagation();
        if lines == 0 {
            return;
        }
        let mode = self.terminal.mode();
        let up = lines > 0;
        let count = lines.unsigned_abs().min(64) as usize;
        if mode.mouse_reporting {
            let report = wheel_report(up, cell_at(&frame, event.position), mode.sgr_mouse);
            self.terminal.write(report.repeat(count));
        } else if mode.alt_screen && mode.alternate_scroll {
            let key = if up { "up" } else { "down" };
            if let Some(seq) = encode_key(key, KeyMods::NONE, mode.app_cursor) {
                self.terminal.write(seq.repeat(count));
            }
        } else {
            self.terminal.scroll_lines(lines);
            cx.notify();
        }
    }
}
