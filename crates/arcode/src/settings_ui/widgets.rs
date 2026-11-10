//! Building blocks of the Settings page and the first-run dialog: rows,
//! switches, steppers, segmented choices and picker buttons (the theme
//! cards are in [`super::theme_card`]).

use std::rc::Rc;

use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_RADIUS, WEIGHT_MEDIUM, type_scale};
use arcode_ui_kit::{IconName, Tooltip, icon, icon_button, switch, theme};
use gpui::{
    App, ClickEvent, Div, ElementId, KeyDownEvent, SharedString, Stateful, Window, div, prelude::*,
    px,
};

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ResetHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Width the label column of a row keeps before its control wraps below.
const LABEL_BASIS: f32 = 220.;

/// What a row knows about the setting it changes: its `config.toml` key
/// (in the label's tooltip, not on the page, and matched by the page
/// filter) and, while the value differs from the default, how to reset it.
pub(crate) struct RowKey {
    pub path: SharedString,
    pub reset: Option<Reset>,
}

/// The reset button of a changed row.
pub(crate) struct Reset {
    /// "Reset to default (4)".
    pub tooltip: SharedString,
    pub click: ResetHandler,
}

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
    row(label.into(), description.into(), None, control, cx)
}

/// A [`setting_row`] for one key of `config.toml`: hovering the label
/// shows the key ("In config.toml: editor.minimap") and, for a changed
/// value, a reset button sits before the control. The filter also matches
/// the key.
pub(crate) fn keyed_setting_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    key: RowKey,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    row(label.into(), description.into(), Some(key), control, cx)
}

fn row(
    label: SharedString,
    description: SharedString,
    key: Option<RowKey>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    let c = &theme(cx).colors;
    let scale = type_scale(cx);
    let path = key.as_ref().map(|k| k.path.clone());
    #[cfg(test)]
    if let Some(path) = &path {
        super::filter::record_key(path);
    }
    if !super::filter::admit(&label, &description, path.as_ref().map(|p| p.as_ref())) {
        return div();
    }
    let selector = format!("setting-row-{label}");
    let reset = key.and_then(|k| k.reset.map(|r| (k.path, r)));
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
                .child(label_text(label, path, scale.body))
                .when(!description.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(description),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .when_some(reset, |el, (path, reset)| {
                    let id = format!("reset-{path}");
                    let selector = id.clone();
                    let click = reset.click;
                    let key_click = click.clone();
                    // A tab stop (Tab reaches it); Enter or Space resets.
                    let focus_ring = c.border_focused;
                    el.child(
                        div()
                            .id(SharedString::from(format!("{id}-focus")))
                            .debug_selector(move || selector)
                            .tab_index(0)
                            .rounded(px(7.))
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus(move |s| s.border_color(focus_ring))
                            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                                if matches!(e.keystroke.key.as_str(), "enter" | "space") {
                                    key_click(window, cx);
                                    // The button goes away: on to the control.
                                    window.focus_next();
                                    cx.stop_propagation();
                                }
                            })
                            .child(
                                icon_button(SharedString::from(id), IconName::RotateCcw)
                                    .size(px(24.))
                                    .tooltip(reset.tooltip)
                                    .on_click(move |_, window, cx| click(window, cx)),
                            ),
                    )
                })
                .child(control),
        )
}

/// A row's label; with the row's `config.toml` key, hovering it shows the
/// key, for people who edit the file (the page itself shows no keys).
fn label_text(label: SharedString, path: Option<SharedString>, size: gpui::Pixels) -> Div {
    let text = div()
        .text_size(size)
        .font_weight(WEIGHT_MEDIUM)
        .child(label);
    let Some(path) = path else {
        return div().flex().child(text);
    };
    let selector = format!("setting-label-{path}");
    div().flex().child(
        text.id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .tooltip(move |_, cx| Tooltip::text(key_tooltip(&path), cx)),
    )
}

/// The tooltip of a row's label: where the setting is in `config.toml`.
pub(crate) fn key_tooltip(path: &str) -> String {
    format!("In config.toml: {path}")
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
