//! Rows of popovers and menus (docs/DESIGN.md 6.12): context menus, the
//! code action menu, the project menu. They go inside a
//! [`crate::popover`], whose 4 px padding keeps them off its rounded
//! corners.

use gpui::{App, Div, ElementId, Stateful, div, prelude::*, px};

use crate::metrics::{ROW_RADIUS, WEIGHT_MEDIUM, type_scale};
use crate::theme;

/// Height of a menu row.
pub const MENU_ROW_H: f32 = 26.;

/// A menu row: 26 tall, radius 6, padding 0 8, gap 8, solid `hover` fill on
/// hover and `pill` fill when `selected` (the keyboard highlight). Append
/// an icon (14, muted), the label, a [`crate::spacer`] and a [`crate::kbd`].
pub fn menu_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let c = &theme(cx).colors;
    let hover = c.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(MENU_ROW_H))
        .px(px(8.))
        .rounded(ROW_RADIUS)
        .text_size(type_scale(cx).body)
        .text_color(c.text)
        .cursor_pointer()
        .when(selected, |el| el.bg(c.pill))
        .when(!selected, |el| el.hover(move |s| s.bg(hover)))
}

/// A 1 px `border` line between groups of a menu, edge to edge across the
/// popover's 4 px padding.
pub fn menu_separator(cx: &App) -> Div {
    div()
        .flex_none()
        .h(px(1.))
        .mx(px(-4.))
        .my(px(4.))
        .bg(theme(cx).colors.border)
}

/// A group title inside a menu ("Recent projects", "Quick fix"): meta size,
/// Medium, muted, sentence case.
pub fn menu_section(text: impl Into<gpui::SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .pt(px(6.))
        .pb(px(2.))
        .text_size(type_scale(cx).meta)
        .font_weight(WEIGHT_MEDIUM)
        .text_color(theme(cx).colors.text_muted)
        .child(text.into())
}
