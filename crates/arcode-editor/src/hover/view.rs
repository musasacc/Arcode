//! The hover popover: diagnostics first, then the documentation, which
//! scrolls when it is long. It opens below the symbol, or above it when
//! the symbol is in the lower part of the window. It is built and placed
//! during the editor element's prepaint, from that frame's layout.

use std::cell::Cell;
use std::rc::Rc;

use arcode_ui_kit::{icon, popover, theme, type_scale, ui_font_family};
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, IntoElement, ParentElement, Pixels, SharedString,
    Styled, Window, canvas, div, point, prelude::*, px, size,
};

use super::{HoverState, anchor_position};
use crate::completion::docs::render_blocks;
use crate::layout::LayoutSnapshot;

/// Widest the popover gets.
const MAX_WIDTH: f32 = 560.;
/// Tallest the documentation gets before it scrolls.
const MAX_DOCS_HEIGHT: f32 = 320.;
/// Distance kept from the window edges.
const WINDOW_MARGIN: Pixels = px(8.);
/// Gap between the line and the popover.
const GAP: Pixels = px(2.);
/// Above this share of the window height the popover opens above the line.
const OPEN_ABOVE_FROM: f32 = 0.6;

/// Lays out the popover for `hover` in `layout` and draws it above the
/// rest of the window; records where it goes in `bounds`. Must be called
/// during prepaint. Draws nothing when the anchor line is not laid out.
pub(crate) fn defer_hover_popover(
    hover: &HoverState,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    layout: &LayoutSnapshot,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((anchor, line_top)) = anchor_position(layout, hover.anchor) else {
        bounds.set(None);
        return;
    };
    let viewport = window.viewport_size();
    let above = line_top > viewport.height * OPEN_ABOVE_FROM;
    let room = if above {
        line_top
    } else {
        viewport.height - anchor.y
    };
    let mut element = popover_element(hover, room, bounds, cx);
    let measured = element.layout_as_root(
        size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        window,
        cx,
    );
    let y = if above {
        line_top - GAP - measured.height
    } else {
        anchor.y + GAP
    };
    let max_x = viewport.width - WINDOW_MARGIN - measured.width;
    let max_y = viewport.height - WINDOW_MARGIN - measured.height;
    let origin = point(
        anchor.x.min(max_x).max(WINDOW_MARGIN),
        y.min(max_y).max(WINDOW_MARGIN),
    );
    window.defer_draw(element, origin, 1);
}

/// The popover itself; `room` is the height available on its side of the
/// line.
fn popover_element(
    hover: &HoverState,
    room: Pixels,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    cx: &App,
) -> AnyElement {
    let colors = &theme(cx).colors;
    let docs_height = px(MAX_DOCS_HEIGHT).min((room - px(80.)).max(px(120.)));
    // A popover (docs/DESIGN.md 6.12); its sections bring their own padding.
    let mut body = popover(cx)
        .id("hover-popover")
        .occlude()
        .p(px(0.))
        .max_w(px(MAX_WIDTH))
        .font_family(ui_font_family(cx))
        .text_size(type_scale(cx).body)
        .text_color(colors.text);
    if !hover.diagnostics.is_empty() {
        let mut rows = div().flex().flex_col().gap_1().px(px(10.)).py(px(8.));
        for d in &hover.diagnostics {
            let color = d.severity.color(colors);
            let mut row = div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .pt(px(2.))
                        .child(icon(d.severity.icon()).size(px(14.)).color(color)),
                )
                .child(div().flex_1().child(SharedString::from(d.message.clone())));
            if !d.source.is_empty() {
                row = row.child(
                    div()
                        .flex_none()
                        .text_size(type_scale(cx).meta)
                        .text_color(colors.text_muted)
                        .child(SharedString::from(d.source.clone())),
                );
            }
            rows = rows.child(row);
        }
        body = body.child(rows);
    }
    if !hover.docs.is_empty() {
        let docs = div()
            .id("hover-docs")
            .max_h(docs_height)
            .overflow_y_scroll()
            .px(px(10.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap_1()
            .when(!hover.diagnostics.is_empty(), |d| {
                d.border_t_1().border_color(colors.border)
            })
            .children(render_blocks(&hover.docs, cx));
        body = body.child(docs);
    }
    let tracker = canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full();
    div()
        .relative()
        .child(body)
        .child(tracker)
        .into_any_element()
}
