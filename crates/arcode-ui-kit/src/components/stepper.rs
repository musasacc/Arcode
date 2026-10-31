//! A numeric stepper (docs/DESIGN.md 6.13): `-` value `+`.

use std::rc::Rc;

use gpui::{App, Div, SharedString, Window, div, prelude::*, px};

use crate::metrics::{CONTROL_H, CONTROL_RADIUS, type_scale};
use crate::{IconName, icon, mono_font_family, theme};

type StepHandler = Rc<dyn Fn(i32, &mut Window, &mut App)>;

/// `value` moved by `delta` steps of `step`, kept in `min..=max` and
/// rounded to the step's precision.
pub fn stepped(value: f32, delta: i32, step: f32, min: f32, max: f32) -> f32 {
    let raw = value + delta as f32 * step;
    let snapped = (raw / step).round() * step;
    let rounded = (snapped * 1000.).round() / 1000.;
    rounded.clamp(min, max)
}

/// `[-] value [+]`: 28 tall, radius 8, 1 px `border_strong`, the value in
/// mono. The buttons (ids `"{id}-dec"` / `"{id}-inc"`) call `change` with
/// -1 or +1.
pub fn stepper(
    id: &str,
    value: impl Into<SharedString>,
    change: impl Fn(i32, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let c = theme(cx).colors.clone();
    let change: StepHandler = Rc::new(change);
    let button = |suffix: &str, glyph: IconName, delta: i32| {
        let change = change.clone();
        div()
            .id(SharedString::from(format!("{id}-{suffix}")))
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(c.hover))
            .child(icon(glyph).size(px(13.)).color(c.text_muted))
            .on_click(move |_, window, cx| change(delta, window, cx))
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .h(CONTROL_H)
        .px(px(1.))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(c.border_strong)
        .child(button("dec", IconName::Minus, -1))
        .child(
            div()
                .min_w(px(40.))
                .text_center()
                .font_family(mono_font_family())
                .text_size(type_scale(cx).readout)
                .text_color(c.text)
                .child(value.into()),
        )
        .child(button("inc", IconName::Plus, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_snap_and_clamp() {
        assert_eq!(stepped(14., 1, 1., 4., 200.), 15.);
        assert_eq!(stepped(1.5, 1, 0.1, 0.5, 4.), 1.6);
        assert_eq!(stepped(1.6, -1, 0.1, 0.5, 4.), 1.5);
        assert_eq!(stepped(4., -1, 1., 4., 200.), 4.);
        assert_eq!(stepped(10_000., 1, 1000., 0., 1_000_000.), 11_000.);
    }
}
