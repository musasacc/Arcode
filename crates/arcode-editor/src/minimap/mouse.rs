//! Minimap mouse handling: a click centers the row under it, a drag (from
//! the slider or after a click) scrolls, and hovering brightens the slider.
//! The wheel needs nothing here: the editor scrolls on wheel events
//! anywhere in its bounds.

use gpui::{Context, MouseButton, MouseDownEvent, Pixels, Point};

use super::ScrollState;
use super::geometry::scroll_centering_row;
use crate::Editor;
use crate::layout::LayoutSnapshot;
use crate::mouse::DragState;

/// A drag that started on the minimap: the mouse y and the editor scroll
/// offset at its start. The scroll follows the mouse by the slider's
/// ratio, so the slider stays under the mouse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MinimapDrag {
    start_y: Pixels,
    start_scroll: Pixels,
}

impl Editor {
    /// Handles a mouse press on the minimap; `false` when it is elsewhere.
    pub(crate) fn minimap_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        layout: &LayoutSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(m) = &layout.minimap else {
            return false;
        };
        let g = &m.geometry;
        let p = event.position;
        if !g.bounds.contains(&p) {
            return false;
        }
        if event.button != MouseButton::Left {
            return true;
        }
        let on_slider = (g.slider.top()..g.slider.bottom()).contains(&p.y);
        if !on_slider {
            let scroll = ScrollState {
                scroll_y: self.scroll_y,
                max_scroll_y: layout.max_scroll_y,
                viewport_h: layout.text_bounds.size.height,
                line_height: layout.line_height,
            };
            self.scroll_y = scroll_centering_row(g.row_at(p.y), scroll);
            self.hide_hover();
        }
        self.drag = Some(DragState::Minimap(MinimapDrag {
            start_y: p.y,
            start_scroll: self.scroll_y,
        }));
        cx.notify();
        true
    }

    /// Scrolls for a minimap drag to window y `y`.
    pub(crate) fn minimap_drag(
        &mut self,
        layout: &LayoutSnapshot,
        drag: MinimapDrag,
        y: Pixels,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = &layout.minimap else {
            return;
        };
        let scroll =
            m.geometry
                .scroll_after_drag(drag.start_scroll, y - drag.start_y, layout.max_scroll_y);
        if scroll != self.scroll_y {
            self.scroll_y = scroll;
            self.hide_hover();
            cx.notify();
        }
    }

    /// Tracks whether the mouse is over the minimap; redraws only when that
    /// changes.
    pub(crate) fn minimap_hover(
        &mut self,
        layout: &LayoutSnapshot,
        p: Point<Pixels>,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let over = hovered
            && layout
                .minimap
                .as_ref()
                .is_some_and(|m| m.geometry.bounds.contains(&p));
        if over != self.minimap.hovered {
            self.minimap.hovered = over;
            cx.notify();
        }
    }
}
