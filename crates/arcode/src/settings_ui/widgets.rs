//! Building blocks of the Settings page and the first-run dialog: rows,
//! switches, steppers, segmented choices and theme swatches.

use std::rc::Rc;

use arcode_config::Theme;
use arcode_ui_kit::{IconName, Tooltip, hsla, icon, switch, theme, truncated};
use gpui::{
    AnyElement, AnyView, App, ClickEvent, Div, ElementId, Hsla, SharedString, Stateful, Window,
    div, prelude::*, px,
};

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Width the label column of a row keeps before its control wraps below.
const LABEL_BASIS: f32 = 220.;

/// A setting: label and description on the left, its control on the right;
/// in a narrow pane the control wraps below the label.
pub fn setting_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    let c = &theme(cx).colors;
    let description: SharedString = description.into();
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_4()
        .gap_y_2()
        .py_2p5()
        .border_b_1()
        .border_color(c.border.opacity(0.5))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .flex_basis(px(LABEL_BASIS))
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().child(label.into()))
                .when(!description.is_empty(), |el| {
                    el.child(div().text_xs().text_color(c.text_muted).child(description))
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
        .h(px(28.))
        .px_2p5()
        .min_w(px(160.))
        .rounded_md()
        .border_1()
        .border_color(c.border)
        .bg(c.surface)
        .cursor_pointer()
        .hover(|s| s.border_color(c.border_focused))
        .text_sm()
        .child(div().flex_1().truncate().child(value.into()))
        .child(
            icon(IconName::ChevronDown)
                .size(px(12.))
                .color(c.text_muted),
        )
        .on_click(move |e, window, cx| click(e, window, cx))
}

/// The colors a swatch shows for a theme.
fn swatch_colors(theme: &Theme) -> [Hsla; 6] {
    let syntax = |name: &str| {
        theme
            .syntax_color(name)
            .map(hsla)
            .unwrap_or(hsla(theme.ui.text))
    };
    [
        hsla(theme.ui.background),
        hsla(theme.ui.title_bar),
        hsla(theme.ui.accent),
        syntax("keyword"),
        syntax("function"),
        hsla(theme.ui.text_muted),
    ]
}

fn swatch_half(theme: &Theme) -> AnyElement {
    let [bg, bar, accent, keyword, function, muted] = swatch_colors(theme);
    let line = |w: f32, color: Hsla| div().h(px(4.)).w(px(w)).rounded_full().bg(color);
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .bg(bg)
        .child(div().h(px(10.)).bg(bar))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_1p5()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(line(14., keyword))
                        .child(line(22., function)),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(line(8., muted))
                        .child(line(26., keyword)),
                )
                .child(line(18., accent)),
        )
        .into_any_element()
}

/// A theme card: a miniature of the theme (two halves for "follow the
/// system") above its name, outlined when `selected`.
pub fn theme_swatch(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    first: Option<&Theme>,
    second: Option<&Theme>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let c = theme(cx).colors.clone();
    let mut preview = div()
        .flex()
        .h(px(52.))
        .w_full()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(c.border);
    for theme in [first, second].into_iter().flatten() {
        preview = preview.child(swatch_half(theme));
    }
    let label: SharedString = label.into();
    let tooltip = label.clone();
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1p5()
        .w(px(124.))
        .p_1p5()
        .rounded_lg()
        .border_2()
        .cursor_pointer()
        .border_color(if selected {
            c.accent
        } else {
            c.surface_raised.opacity(0.)
        })
        .hover(|s| s.bg(c.hover))
        // Long names end in `…` on the card; the tooltip has all of it.
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip.clone(), cx) })
        .child(preview)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .text_xs()
                .when(selected, |el| {
                    el.child(icon(IconName::Check).size(px(12.)).color(c.accent))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .debug_selector(|| "theme-card-label".into())
                        .child(truncated(label)),
                ),
        )
}
