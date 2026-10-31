//! A segmented control (docs/DESIGN.md 6.13): mutually exclusive choices
//! in one 28 px outlined frame.

use std::rc::Rc;

use gpui::{App, Div, SharedString, Window, div, prelude::*, px};

use crate::metrics::{CONTROL_H, CONTROL_RADIUS, WEIGHT_SEMIBOLD, type_scale};
use crate::theme;

/// A row of mutually exclusive choices: 28 tall, radius 8, 1 px
/// `border_strong` outline, 2 px inner padding; segments are 22 tall with
/// radius 6, the selected one `pill` filled and SemiBold. `select` gets the
/// clicked index. Segment ids are `"{id}-{index}"`.
pub fn segmented(
    id: &str,
    labels: &[&str],
    selected: usize,
    select: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let c = theme(cx).colors.clone();
    let size = type_scale(cx).body;
    let select = Rc::new(select);
    let mut row = div()
        .flex()
        .flex_none()
        .items_center()
        .h(CONTROL_H)
        .p(px(2.))
        .gap(px(2.))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(c.border_strong)
        .text_size(size);
    for (ix, label) in labels.iter().enumerate() {
        let on = ix == selected;
        let select = select.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("{id}-{ix}")))
                .flex()
                .items_center()
                .h(px(22.))
                .px(px(10.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |el| {
                    el.bg(c.pill)
                        .text_color(c.text)
                        .font_weight(WEIGHT_SEMIBOLD)
                })
                .when(!on, |el| {
                    el.text_color(c.text_muted)
                        .hover(|s| s.bg(c.hover).text_color(c.text))
                })
                .child(SharedString::from(label.to_string()))
                .on_click(move |_, window, cx| select(ix, window, cx)),
        );
    }
    row
}
