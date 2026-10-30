//! Mouse handling for extension points: gutter lane clicks, code lens
//! clicks and hover, and popover placement.

use arcode_buffer::Point as BufPoint;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Context, Corner, IntoElement, MouseDownEvent,
    ParentElement, Pixels, Point, Styled, Window, anchored, div, point, px, size,
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

    /// Elements for the visible popovers, positioned for `layout`.
    pub(crate) fn popover_elements(&self, layout: &LayoutSnapshot) -> Vec<AnyElement> {
        let mut out = Vec::new();
        for (_, popover) in &self.ext.popovers {
            let p = popover.anchor;
            if layout.visible(p.line).is_none() {
                continue;
            }
            let x = layout.text_left() + layout.x_for_glyph(p.line, p.column);
            let top = layout.line_top(p.line);
            let (pos, corner, pad) = match popover.placement {
                PopoverPlacement::Below => {
                    (point(x, top + layout.line_height), Corner::TopLeft, true)
                }
                PopoverPlacement::Above => (point(x, top), Corner::BottomLeft, false),
            };
            let body = if pad {
                div().pt(px(2.)).child(popover.content.clone())
            } else {
                div().pb(px(2.)).child(popover.content.clone())
            };
            out.push(
                anchored()
                    .position(pos)
                    .anchor(corner)
                    .snap_to_window_with_margin(px(8.))
                    .child(body)
                    .into_any_element(),
            );
        }
        out
    }
}

/// Lays out the popovers and lets them draw over the rest of the window.
/// Must be called during prepaint.
pub(crate) fn defer_popovers(elements: Vec<AnyElement>, window: &mut Window, cx: &mut App) {
    for mut el in elements {
        el.layout_as_root(AvailableSpace::min_size(), window, cx);
        window.defer_draw(el, point(Pixels::ZERO, Pixels::ZERO), 2);
    }
}
