//! Building blocks of the Settings page and the first-run dialog: rows,
//! switches, steppers, segmented choices and picker buttons (the theme
//! cards are in [`super::theme_card`]).

use std::rc::Rc;

use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_RADIUS, WEIGHT_MEDIUM, type_scale};
use arcode_ui_kit::{IconName, icon, switch, theme};
use gpui::{App, ClickEvent, Div, ElementId, SharedString, Stateful, Window, div, prelude::*, px};

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Width the label column of a row keeps before its control wraps below.
const LABEL_BASIS: f32 = 220.;

/// A setting (docs/DESIGN.md 6.14): label (body, Medium) and description
/// (meta, muted) on the left, its control on the right, 14 px of padding
/// above and below and a hairline under it; in a narrow pane the control
/// wraps below the label. Left out (an empty element) when the page's
/// filter does not match it.
pub fn setting_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    let c = &theme(cx).colors;
    let scale = type_scale(cx);
    let label: SharedString = label.into();
    let description: SharedString = description.into();
    if !super::filter::admit(&label, &description) {
        return div();
    }
    let selector = format!("setting-row-{label}");
    div()
        .debug_selector(move || selector)
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_4()
        .gap_y_2()
        .py(px(14.))
        .border_b_1()
        .border_color(c.border)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .flex_basis(px(LABEL_BASIS))
                .min_w_0()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(scale.body)
                        .font_weight(WEIGHT_MEDIUM)
                        .child(label),
                )
                .when(!description.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(description),
                    )
                }),
        )
        .child(div().flex_none().child(control))
}

/// A switch that runs `toggle` with the new state when clicked.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    switch(id, on).on_click(move |_, window, cx| toggle(!on, window, cx))
}

// The stepper and segmented control moved to the kit (Ember/Chalk look).
pub use arcode_ui_kit::{segmented, stepped, stepper};

/// A button showing the current value of a picked setting.
pub fn picker_button(
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let c = theme(cx).colors.clone();
    let click: Handler = Rc::new(click);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(CONTROL_H)
        .px(px(10.))
        .min_w(px(160.))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(c.border_strong)
        .bg(c.panel())
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .text_size(type_scale(cx).body)
        .child(div().flex_1().truncate().child(value.into()))
        .child(
            icon(IconName::ChevronDown)
                .size(px(14.))
                .color(c.text_muted),
        )
        .on_click(move |e, window, cx| click(e, window, cx))
}
