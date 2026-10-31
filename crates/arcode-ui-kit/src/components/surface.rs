//! Surfaces of the Ember/Chalk workbench (docs/DESIGN.md 5, 6.4, 6.12):
//! floating panels on the canvas, their title rows and section headers,
//! inset list rows, overlays and inputs.

use gpui::{
    AbsoluteLength, App, Div, ElementId, FontWeight, Hsla, Pixels, SharedString, Stateful, Styled,
    div, prelude::*, px,
};

use crate::metrics::{
    OVERLAY_RADIUS, PANEL_RADIUS, PANEL_TITLE_H, POPOVER_RADIUS, ROW_H, ROW_INSET, ROW_RADIUS,
    SECTION_H, WEIGHT_SEMIBOLD, type_scale,
};
use crate::{Elevated, theme};

/// The edge color of a panel: `border_focused` when it holds focus, else
/// the hairline `border`. Always 1 px, so focus never moves anything.
pub fn panel_border(focused: bool, cx: &App) -> Hsla {
    let c = &theme(cx).colors;
    if focused { c.border_focused } else { c.border }
}

/// A floating panel: `panel` fill, 1 px edge (amber when `focused`),
/// radius 10, children clipped. Panels never have shadows.
pub fn panel(focused: bool, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .min_h_0()
        .bg(theme(cx).colors.panel())
        .border_1()
        .border_color(panel_border(focused, cx))
        .rounded(PANEL_RADIUS)
        .overflow_hidden()
}

/// The title row of a sidebar panel: 40 tall, title in the title size
/// SemiBold, an optional muted `subtitle` (empty for none), then a flexible
/// space; append icon buttons (24 px, radius 7) as children.
pub fn panel_title(
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let c = &theme(cx).colors;
    let scale = type_scale(cx);
    let subtitle: SharedString = subtitle.into();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(PANEL_TITLE_H)
        .pl(px(14.))
        .pr(px(8.))
        .child(
            div()
                .flex_none()
                .text_size(scale.title)
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(c.text)
                .child(title.into()),
        )
        .when(!subtitle.is_empty(), |el| {
            el.child(
                div()
                    .ml(px(4.))
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(scale.meta)
                    .text_color(c.text_muted)
                    .child(crate::truncated(subtitle)),
            )
        })
        .child(spacer())
}

/// A section header inside a panel ("Call stack", "Breakpoints"): 30 tall,
/// meta size SemiBold in `text`, sentence case (never uppercased). Append a
/// [`crate::badge`], a muted suffix, [`spacer`] and buttons as children.
pub fn section_header(text: impl Into<SharedString>, cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(SECTION_H)
        .px(px(12.))
        .text_size(type_scale(cx).meta)
        .text_color(c.text)
        .child(
            div()
                .flex_none()
                .font_weight(WEIGHT_SEMIBOLD)
                .child(text.into()),
        )
}

/// A flexible empty space that pushes later children to the right.
pub fn spacer() -> Div {
    div().flex_1().min_w_0()
}

/// A list or tree row chip: 24 tall, inset 6 px from the panel edges,
/// radius 6, solid `hover` fill on hover and `pill` fill when `selected`.
/// Change the height with `.h(..)` (22 in debugger columns, 26 in menus).
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let c = &theme(cx).colors;
    let hover = c.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(ROW_H)
        .mx(ROW_INSET)
        .px(px(8.))
        .rounded(ROW_RADIUS)
        .text_size(type_scale(cx).body)
        .cursor_pointer()
        .when(selected, |el| {
            el.bg(c.pill)
                .text_color(c.text)
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!selected, |el| el.hover(move |s| s.bg(hover)))
}

/// The palette and dialogs: `panel_raised` fill, 1 px `border_strong`
/// edge, radius 12 and the large overlay shadow (none in low-end mode).
pub fn overlay(cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .flex_col()
        .bg(c.panel_raised())
        .border_1()
        .border_color(c.border_strong)
        .rounded(OVERLAY_RADIUS)
        .overflow_hidden()
        .elevated_lg(cx)
}

/// Popovers and menus: `panel_raised` fill, 1 px `border_strong` edge,
/// radius 10, 4 px padding and the small overlay shadow. Rows inside are
/// [`list_row`]s with `.mx(px(0.)).h(px(26.))`.
pub fn popover(cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .flex_col()
        .p(px(4.))
        .bg(c.panel_raised())
        .border_1()
        .border_color(c.border_strong)
        .rounded(POPOVER_RADIUS)
        .elevated(cx)
}

/// The frame of a text input: 28 tall, radius 8, `panel` fill (pass
/// `on_raised` on overlays to use `canvas`), 1 px `border_strong` edge that
/// turns `accent` while `focused`. No glow.
pub fn input_frame(focused: bool, on_raised: bool, cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(crate::metrics::CONTROL_H)
        .px(px(8.))
        .rounded(crate::metrics::CONTROL_RADIUS)
        .bg(if on_raised { c.canvas } else { c.panel() })
        .border_1()
        .border_color(if focused { c.accent } else { c.border_strong })
        .text_size(type_scale(cx).body)
}

/// Sets every border side of `el` to `width` (GPUI's helpers only offer
/// whole pixel steps; the switch and the unsaved ring use 1.5 px).
pub fn border_width<E: Styled>(mut el: E, width: Pixels) -> E {
    let width = Some(AbsoluteLength::Pixels(width));
    let edges = &mut el.style().border_widths;
    edges.top = width;
    edges.right = width;
    edges.bottom = width;
    edges.left = width;
    el
}
