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
use crate::layout::{LayoutSnapshot, MARKER_WIDTH};

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
        if self.bulb_hit(layout, p) {
            self.show_code_actions(false, cx);
            return true;
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

    /// A right click in the gutter: emits
    /// [`EditorEvent::GutterContextClicked`] for a mark, or for an empty
    /// slot of a lane that is clickable when empty.
    pub(crate) fn gutter_context_click(
        &mut self,
        event: &MouseDownEvent,
        layout: &LayoutSnapshot,
        cx: &mut Context<Self>,
    ) {
        let p = event.position;
        if p.x < layout.bounds.left() {
            return;
        }
        let Some(lane) = layout.gutter.lane_at(p.x - layout.bounds.left()) else {
            return;
        };
        let RowKind::Text(line) = layout.row_at(p.y, self.buffer.line_count()) else {
            return;
        };
        if p.y > layout.line_top(line) + layout.line_height {
            return;
        }
        let tag = self.gutter_tag(line, lane);
        if tag.is_some() || lane.clickable_when_empty() {
            cx.emit(EditorEvent::GutterContextClicked { line, lane, tag });
        }
    }

    /// Whether `p` is on the light bulb (the whole marker column cell of
    /// its line). A run button wins the shared column on its line, so the
    /// bulb is neither drawn nor clickable there.
    pub(crate) fn bulb_hit(&self, layout: &LayoutSnapshot, p: Point<Pixels>) -> bool {
        let Some((line, _)) = self.lang.actions.bulb else {
            return false;
        };
        if layout.visible(line).is_none() || self.gutter_tag(line, GutterLane::Runnables).is_some()
        {
            return false;
        }
        let cell = crate::gutter::bulb_bounds(layout, line, layout.line_height);
        let left = layout.bounds.left() + layout.gutter.marker_left;
        let cell = Bounds::new(
            point(left, cell.top()),
            size(MARKER_WIDTH, cell.size.height),
        );
        cell.contains(&p)
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
        out.extend(self.link_bounds(layout));
        if let Some((line, _)) = self.lang.actions.bulb
            && layout.visible(line).is_some()
        {
            let top = layout.line_top(line);
            let left = layout.bounds.left() + layout.gutter.marker_left;
            out.push(Bounds::new(
                point(left, top),
                size(MARKER_WIDTH, layout.line_height),
            ));
        }
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
                side: popover
                    .side
                    .clone()
                    .map(|side| div().child(side).into_any_element()),
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
    side: Option<AnyElement>,
    x: Pixels,
    line_top: Pixels,
    line_height: Pixels,
    placement: PopoverPlacement,
}

/// Distance kept from the window edges.
const WINDOW_MARGIN: Pixels = px(8.);
/// Gap between the anchor line and the popover.
const ANCHOR_GAP: Pixels = px(2.);
/// Gap between a popover and its side panel.
const SIDE_GAP: Pixels = px(4.);

/// The top-left corner for a popover of `size` attached to the line at
/// `line_top`: on its preferred side, or the other side when it does not
/// fit there but does (or fits better) on the other; shifted left to stay
/// inside the window. `fit_height` is what must fit on a side: the popover
/// and its side panel, which is aligned with it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_popover(
    size: Size<Pixels>,
    fit_height: Pixels,
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
    let fit = fit_height.max(size.height);
    let fits_below = fit <= room_below;
    let fits_above = fit <= room_above;
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

/// The top-left corner of a side panel of `size` next to a popover at
/// `main`: right of it, or left when it does not fit there; top-aligned
/// when the popover is below its line (`main` starts below `line_top`),
/// bottom-aligned when above, so the panel never pushes the popover away
/// from the line. Kept inside the window.
pub(crate) fn place_side(
    size: Size<Pixels>,
    main: Bounds<Pixels>,
    line_top: Pixels,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let right = main.right() + SIDE_GAP;
    let left = main.left() - SIDE_GAP - size.width;
    let max_x = viewport.width - WINDOW_MARGIN - size.width;
    let x = if right <= max_x || left < WINDOW_MARGIN {
        right.min(max_x).max(WINDOW_MARGIN)
    } else {
        left
    };
    let max_y = viewport.height - WINDOW_MARGIN - size.height;
    let y = if main.top() > line_top {
        main.top()
    } else {
        main.bottom() - size.height
    };
    point(x, y.min(max_y).max(WINDOW_MARGIN))
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
        let side = slot.side.map(|mut side| {
            let side_size = side.layout_as_root(
                gpui::size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                window,
                cx,
            );
            (side, side_size)
        });
        let side = side.filter(|(_, s)| s.width > px(0.) && s.height > px(0.));
        let fit_height = side.as_ref().map_or(size.height, |(_, s)| s.height);
        let origin = place_popover(
            size,
            fit_height,
            slot.x,
            slot.line_top,
            slot.line_height,
            slot.placement,
            viewport,
        );
        window.defer_draw(slot.element, origin, 2);
        if let Some((side, side_size)) = side {
            let main = Bounds::new(origin, size);
            let at = place_side(side_size, main, slot.line_top, viewport);
            window.defer_draw(side, at, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(placement: PopoverPlacement, line_top: f32, height: f32) -> Point<Pixels> {
        place_popover(
            size(px(300.), px(height)),
            px(0.),
            px(900.),
            px(line_top),
            px(20.),
            placement,
            size(px(1000.), px(600.)),
        )
    }

    #[test]
    fn a_tall_side_panel_flips_a_short_popover() {
        // 40 px fit below the line, the 150 px side panel next to it not:
        // both go above, the popover's bottom at the line.
        let origin = place_popover(
            size(px(300.), px(40.)),
            px(150.),
            px(100.),
            px(500.),
            px(20.),
            PopoverPlacement::Below,
            size(px(1000.), px(600.)),
        );
        assert_eq!(origin, point(px(100.), px(458.)));
    }

    #[test]
    fn side_panels_align_with_the_edge_nearest_the_line() {
        let viewport = size(px(1000.), px(600.));
        let side = size(px(200.), px(150.));
        // Below the line: right of the list, tops aligned.
        let main = Bounds::new(point(px(100.), px(122.)), size(px(300.), px(60.)));
        assert_eq!(
            place_side(side, main, px(100.), viewport),
            point(px(404.), px(122.))
        );
        // Above the line: bottoms aligned, so the list stays at the line.
        let main = Bounds::new(point(px(100.), px(438.)), size(px(300.), px(60.)));
        assert_eq!(
            place_side(side, main, px(500.), viewport),
            point(px(404.), px(348.))
        );
        // No room on the right: left of the list.
        let main = Bounds::new(point(px(650.), px(122.)), size(px(300.), px(60.)));
        assert_eq!(
            place_side(side, main, px(100.), viewport),
            point(px(446.), px(122.))
        );
        // Taller than the room above the list's bottom: kept in the window.
        let main = Bounds::new(point(px(100.), px(20.)), size(px(300.), px(60.)));
        assert_eq!(place_side(side, main, px(100.), viewport).y, px(8.));
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
