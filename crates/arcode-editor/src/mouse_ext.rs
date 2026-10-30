//! Mouse handling for extension points: gutter lane clicks, code lens
//! clicks and hover, and popover placement.

use arcode_buffer::Point as BufPoint;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Context, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Size, Window, div, point, px, size,
};

use crate::Editor;
use crate::EditorEvent;
use crate::ext::{GutterLane, PopoverPlacement, RowKind};
use crate::layout::LayoutSnapshot;

impl Editor {
    /// Handles a click on a lens item or a gutter lane. Returns `true` if
    /// the click was consumed (no selection change).
    pub(crate) fn mouse_down_ext(
        &mut self,
        event: &MouseDownEvent,
        layout: &LayoutSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        let p = event.position;
        if let Some(hit) = layout.lens_hits.iter().find(|h| h.bounds.contains(&p)) {
            let (set, line, id) = hit.target.clone();
            cx.emit(EditorEvent::CodeLensClicked { set, line, id });
            return true;
        }
        if p.x >= layout.text_bounds.left() || p.x < layout.bounds.left() {
            return false;
        }
        let Some(lane) = layout.gutter.lane_at(p.x - layout.bounds.left()) else {
            return false;
        };
        let RowKind::Text(line) = layout.row_at(p.y, self.buffer.line_count()) else {
            return true;
        };
        if p.y > layout.line_top(line) + layout.line_height {
            return lane.clickable_when_empty();
        }
        let tag = self.gutter_tag(line, lane);
        if tag.is_none() && !lane.clickable_when_empty() {
            return false;
        }
        cx.emit(EditorEvent::GutterClicked {
            line,
            lane,
            tag,
            modifiers: event.modifiers,
        });
        true
    }

    /// Tag of the first mark on `line` in `lane`.
    pub(crate) fn gutter_tag(&self, line: usize, lane: GutterLane) -> Option<u64> {
        self.ext
            .gutter_in(line..line + 1)
            .into_iter()
            .find(|m| m.lane == lane)
            .map(|m| m.tag)
    }

    /// Updates the hovered lens item; returns whether it changed.
    pub(crate) fn hover_lens(&mut self, layout: &LayoutSnapshot, p: Point<Pixels>) -> bool {
        let hovered = layout
            .lens_hits
            .iter()
            .find(|h| h.bounds.contains(&p))
            .map(|h| h.target.clone());
        if hovered != self.ext.hovered_lens {
            self.ext.hovered_lens = hovered;
            return true;
        }
        false
    }

    /// Areas that get a pointing-hand cursor: lens items, the breakpoint
    /// lane and gutter marks.
    pub(crate) fn clickable_bounds(&self, layout: &LayoutSnapshot) -> Vec<Bounds<Pixels>> {
        let mut out: Vec<Bounds<Pixels>> = layout.lens_hits.iter().map(|h| h.bounds).collect();
        let left = layout.bounds.left();
        for &(lane, lane_left, width) in &layout.gutter.lanes {
            if lane.clickable_when_empty() {
                out.push(Bounds::new(
                    point(left + lane_left, layout.bounds.top()),
                    size(width, layout.bounds.size.height),
                ));
                continue;
            }
            for mark in self.ext.gutter_in(layout.visible_range()) {
                if mark.lane == lane {
                    out.push(Bounds::new(
                        point(left + lane_left, layout.line_top(mark.line)),
                        size(width, layout.line_height),
                    ));
                }
            }
        }
        out
    }

    /// Window position of the caret at buffer point `p`, if its line is
    /// laid out: the top-left of the caret cell and the line height.
    pub fn position_for_point(&self, p: BufPoint) -> Option<(Point<Pixels>, Pixels)> {
        let layout = self.layout.as_ref()?;
        layout.visible(p.line)?;
        let x = layout.text_left() + layout.x_for_glyph(p.line, p.column);
        Some((point(x, layout.line_top(p.line)), layout.line_height))
    }

    /// The visible popovers with where they attach in `layout`.
    pub(crate) fn popover_elements(&self, layout: &LayoutSnapshot) -> Vec<PopoverSlot> {
        let mut out = Vec::new();
        for (_, popover) in &self.ext.popovers {
            let p = popover.anchor;
            if layout.visible(p.line).is_none() {
                continue;
            }
            out.push(PopoverSlot {
                element: div().child(popover.content.clone()).into_any_element(),
                x: layout.text_left() + layout.x_for_glyph(p.line, p.column),
                line_top: layout.line_top(p.line),
                line_height: layout.line_height,
                placement: popover.placement,
            });
        }
        out
    }
}

/// A popover element and the line it attaches to (window coordinates).
pub(crate) struct PopoverSlot {
    element: AnyElement,
    x: Pixels,
    line_top: Pixels,
    line_height: Pixels,
    placement: PopoverPlacement,
}

/// Distance kept from the window edges.
const WINDOW_MARGIN: Pixels = px(8.);
/// Gap between the anchor line and the popover.
const ANCHOR_GAP: Pixels = px(2.);

/// The top-left corner for a popover of `size` attached to the line at
/// `line_top`: on its preferred side, or the other side when it does not
/// fit there but does (or fits better) on the other; shifted left to stay
/// inside the window.
pub(crate) fn place_popover(
    size: Size<Pixels>,
    x: Pixels,
    line_top: Pixels,
    line_height: Pixels,
    placement: PopoverPlacement,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let below = line_top + line_height + ANCHOR_GAP;
    let above = line_top - ANCHOR_GAP - size.height;
    let room_below = viewport.height - WINDOW_MARGIN - below;
    let room_above = line_top - ANCHOR_GAP - WINDOW_MARGIN;
    let fits_below = size.height <= room_below;
    let fits_above = size.height <= room_above;
    let use_below = match placement {
        PopoverPlacement::Below => fits_below || (!fits_above && room_below >= room_above),
        PopoverPlacement::Above => !fits_above && (fits_below || room_below > room_above),
    };
    let y = if use_below {
        below
    } else {
        above.max(WINDOW_MARGIN)
    };
    let max_x = viewport.width - WINDOW_MARGIN - size.width;
    point(x.min(max_x).max(WINDOW_MARGIN), y)
}

/// Lays out the popovers, places them (flipping to the other side of the
/// anchor line when there is no room) and lets them draw over the rest of
/// the window. Must be called during prepaint.
pub(crate) fn defer_popovers(slots: Vec<PopoverSlot>, window: &mut Window, cx: &mut App) {
    let viewport = window.viewport_size();
    for mut slot in slots {
        let size = slot.element.layout_as_root(
            size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            window,
            cx,
        );
        let origin = place_popover(
            size,
            slot.x,
            slot.line_top,
            slot.line_height,
            slot.placement,
            viewport,
        );
        window.defer_draw(slot.element, origin, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(placement: PopoverPlacement, line_top: f32, height: f32) -> Point<Pixels> {
        place_popover(
            size(px(300.), px(height)),
            px(900.),
            px(line_top),
            px(20.),
            placement,
            size(px(1000.), px(600.)),
        )
    }

    #[test]
    fn popovers_flip_when_there_is_no_room() {
        // Room below: stays below; x is pulled inside the window.
        assert_eq!(
            at(PopoverPlacement::Below, 100., 200.),
            point(px(692.), px(122.))
        );
        // Near the bottom: flips above the line.
        assert_eq!(
            at(PopoverPlacement::Below, 500., 200.),
            point(px(692.), px(298.))
        );
        // Above the first lines there is no room: below instead.
        assert_eq!(
            at(PopoverPlacement::Above, 30., 100.),
            point(px(692.), px(52.))
        );
        assert_eq!(
            at(PopoverPlacement::Above, 300., 100.),
            point(px(692.), px(198.))
        );
    }
}
