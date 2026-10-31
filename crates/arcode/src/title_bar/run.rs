//! The right cluster's debugger-aware parts (docs/DESIGN.md 6.2): the
//! Paused chip and the Run split button, which is an outline while idle and
//! becomes the solid Continue button (the only amber block) while paused.

use arcode_ui_kit::metrics::{
    CONTROL_H, CONTROL_RADIUS, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, icon_size,
};
use arcode_ui_kit::{IconName, Tooltip, icon, readout, theme, truncated, type_scale};
use gpui::{AnyView, App, Context, IntoElement, SharedString, div, prelude::*, px};

use crate::actions::{RunProject, shortcut_label};
use crate::debugger::actions::{Continue, PickRunCommand};
use crate::prefs::tour::TourTarget;
use crate::workspace::Workspace;

/// Longest command shown next to "Run" before it truncates.
const COMMAND_MAX_W: f32 = 180.;

/// The Paused chip: `paused_line` fill, a `pause` icon in `accent`,
/// "Paused" and the location in mono. It keeps the state visible while the
/// bottom panel is hidden.
pub fn paused_chip(location: SharedString, cx: &App) -> impl IntoElement + use<> {
    let c = &theme(cx).colors;
    let scale = type_scale(cx);
    div()
        .id("paused-chip")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(c.paused_line)
        .text_size(scale.meta)
        .text_color(c.text)
        .child(icon(IconName::Pause).size(px(13.)).color(c.accent))
        .child(div().font_weight(WEIGHT_SEMIBOLD).child("Paused"))
        .when(!location.is_empty(), |el| el.child(readout(location)))
        .tooltip(|_, cx| -> AnyView { Tooltip::text("The debugger is paused", cx) })
}

/// Tooltip of the idle Run part.
fn run_tooltip(command: Option<&str>) -> String {
    match command {
        Some(cmd) => format!("Run `{cmd}` ({})", shortcut_label("secondary-f5")),
        None => "No run command detected; pick one from the menu".to_string(),
    }
}

/// The Run split button. Idle: `panel` fill, 1 px `border`, an `accent`
/// play icon, "Run", the command in muted, a hairline and the menu chevron.
/// Paused: a solid `accent` Continue with `accent_text` content.
pub fn run_split(
    ws: &Workspace,
    paused: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let command = ws.run_command_line();
    let anchors = ws.tour_anchors().clone();
    let (fill, hover, fg, line) = if paused {
        (c.accent, c.accent_hover, c.accent_text, c.accent_text)
    } else {
        (c.panel(), c.hover, c.text, c.border)
    };
    let id = if paused { "continue" } else { "run" };
    let main = div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .gap(px(7.))
        .h_full()
        .min_w_0()
        .pl(px(10.))
        .pr(px(9.))
        .rounded_l(CONTROL_RADIUS - px(1.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .map(|el| {
            if paused {
                el.child(
                    icon(IconName::CirclePlay)
                        .size(icon_size::TOOLBAR)
                        .color(fg),
                )
                .child(div().font_weight(WEIGHT_MEDIUM).child("Continue"))
                .tooltip(|_, cx| -> AnyView {
                    Tooltip::text(format!("Continue ({})", shortcut_label("f5")), cx)
                })
                .on_click(|_, window, cx| window.dispatch_action(Box::new(Continue), cx))
            } else {
                let tip = run_tooltip(command.as_deref());
                el.child(
                    icon(IconName::Play)
                        .size(icon_size::TOOLBAR)
                        .color(c.accent),
                )
                .child(div().font_weight(WEIGHT_MEDIUM).child("Run"))
                .when_some(command.clone(), |el, cmd| {
                    el.child(
                        div()
                            .min_w_0()
                            .max_w(px(COMMAND_MAX_W))
                            .overflow_hidden()
                            .text_color(c.text_muted)
                            .child(truncated(cmd)),
                    )
                })
                .tooltip(move |_, cx| -> AnyView { Tooltip::text(tip.clone(), cx) })
                .on_click(|_, window, cx| window.dispatch_action(Box::new(RunProject), cx))
            }
        });
    let chevron = div()
        .id("run-menu")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(26.))
        .h_full()
        .rounded_r(CONTROL_RADIUS - px(1.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(
            icon(IconName::ChevronDown)
                .size(icon_size::TOOLBAR)
                .color(if paused { fg } else { c.text_muted }),
        )
        .tooltip(|_, cx| -> AnyView { Tooltip::text("Pick a run command or edit it", cx) })
        .on_click(|_, window, cx| window.dispatch_action(Box::new(PickRunCommand), cx));
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(CONTROL_H)
        .max_w(px(COMMAND_MAX_W + 120.))
        .rounded(CONTROL_RADIUS)
        .bg(fill)
        .border_1()
        .border_color(if paused { fill } else { c.border })
        .text_size(scale.body)
        .text_color(fg)
        .child(anchors.recorder(TourTarget::RunButton))
        .child(main)
        .child(div().flex_none().w(px(1.)).h(px(16.)).bg(line))
        .child(chevron)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_tooltip_names_the_command() {
        assert!(run_tooltip(Some("cargo run")).starts_with("Run `cargo run` ("));
        assert!(run_tooltip(None).starts_with("No run command"));
    }
}
