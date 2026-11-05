//! The colour picker popover: a saturation/value square, a hue strip, an
//! alpha strip and a hex field, drawn with GPUI gradients. A drag updates
//! the token on every move and re-themes the app once per frame.

use arcode_config::theme::Hsv;
use arcode_ui_kit::{hsla, input_frame, mono_font_family, popover, theme, type_scale};
use gpui::{
    AnyElement, Bounds, Context, Corner, CursorStyle, DispatchPhase, Focusable, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, anchored,
    canvas, deferred, div, linear_color_stop, linear_gradient, point, prelude::*, px,
};

use super::ThemeEditor;

/// Width of the picker's areas.
const W: f32 = 232.;
/// Height of the saturation/value square.
const SQUARE_H: f32 = 148.;
/// Height of the hue and alpha strips.
const STRIP_H: f32 = 14.;
/// About how tall the whole picker is.
const PICKER_H: f32 = 300.;

/// A draggable part of the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Area {
    /// Saturation (x) and value (y).
    Square,
    /// Hue (x).
    Hue,
    /// Alpha (x).
    Alpha,
}

impl Area {
    /// `hsv` with the pointer at the fractions `fx`, `fy` (`0..=1` from the
    /// top left) of this area.
    pub(crate) fn pick(self, hsv: Hsv, fx: f32, fy: f32) -> Hsv {
        let (fx, fy) = (fx.clamp(0., 1.), fy.clamp(0., 1.));
        match self {
            Self::Square => Hsv {
                s: fx,
                v: 1. - fy,
                ..hsv
            },
            // The right end is 360°, the same red as the left end.
            Self::Hue => Hsv {
                h: (fx * 360.).min(359.99),
                ..hsv
            },
            Self::Alpha => Hsv { a: fx, ..hsv },
        }
    }
}

fn fractions(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
    let x = (position.x - bounds.origin.x) / bounds.size.width.max(px(1.));
    let y = (position.y - bounds.origin.y) / bounds.size.height.max(px(1.));
    (x, y)
}

impl ThemeEditor {
    /// The pointer went down in (`start`) or moved over an area while
    /// dragging it.
    fn picker_pointer(
        &mut self,
        area: Area,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if start {
            picker.drag = Some(area);
        } else if picker.drag != Some(area) {
            return;
        }
        let (fx, fy) = fractions(bounds, position);
        picker.hsv = area.pick(picker.hsv, fx, fy);
        let color = picker.hsv.to_color();
        let token = picker.token.clone();
        picker
            .hex
            .update(cx, |field, cx| field.set_text(&color.to_hex(), cx));
        self.set_color_coalesced(&token, color, window, cx);
    }

    fn picker_release(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.as_mut()
            && picker.drag.take().is_some()
        {
            cx.notify();
        }
    }

    /// Text typed into the picker's hex field: applied once it is a colour.
    pub(super) fn picker_hex_typed(&mut self, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let Ok(color) = text.parse::<arcode_config::Color>() else {
            return;
        };
        // The field echoing a drag: keep the dragged hue.
        if color == picker.hsv.to_color() {
            return;
        }
        picker.hsv = Hsv::from_color(color);
        let token = picker.token.clone();
        self.set_color(&token, color, cx);
    }

    /// The listeners of a draggable area, registered while painting.
    fn area_events(&self, area: Area, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let down = this.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && e.button == MouseButton::Left
                        && bounds.contains(&e.position)
                    {
                        down.update(cx, |editor, cx| {
                            editor.picker_pointer(area, bounds, e.position, true, window, cx)
                        })
                        .ok();
                    }
                });
                let moved = this.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && e.pressed_button == Some(MouseButton::Left)
                    {
                        moved
                            .update(cx, |editor, cx| {
                                editor.picker_pointer(area, bounds, e.position, false, window, cx)
                            })
                            .ok();
                    }
                });
                let up = this.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        up.update(cx, |editor, cx| editor.picker_release(cx)).ok();
                    }
                });
            },
        )
        // Without an inset an absolute child sits after its in-flow
        // siblings, below the strip it should cover.
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// The open picker, drawn over the page.
    pub(super) fn render_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let c = theme(cx).colors.clone();
        let hsv = picker.hsv;
        let hue: Hsla = hsla(hsv.hue_color());
        let opaque = hsla(hsv.to_color().with_alpha(255));
        let before = self
            .edit
            .start()
            .token_color(&picker.token)
            .map(hsla)
            .unwrap_or(c.text);
        let now = hsla(hsv.to_color());
        let marker = |x: f32, y: f32, size: f32| {
            div()
                .absolute()
                .left(px(x - size / 2.))
                .top(px(y - size / 2.))
                .size(px(size))
                .rounded_full()
                .border_2()
                .border_color(gpui::white())
                .shadow_sm()
        };
        let strip_marker = |x: f32| {
            div()
                .absolute()
                .left(px((x * W - 3.).clamp(0., W - 6.)))
                .top(px(-2.))
                .w(px(6.))
                .h(px(STRIP_H + 4.))
                .rounded(px(3.))
                .border_2()
                .border_color(gpui::white())
                .shadow_sm()
        };
        let square = div()
            .id("picker-square")
            .relative()
            .w(px(W))
            .h(px(SQUARE_H))
            .rounded(px(6.))
            .cursor(CursorStyle::Crosshair)
            .bg(linear_gradient(
                90.,
                linear_color_stop(gpui::white(), 0.),
                linear_color_stop(hue, 1.),
            ))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(6.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(gpui::transparent_black(), 0.),
                        linear_color_stop(gpui::black(), 1.),
                    )),
            )
            .child(marker(hsv.s * W, (1. - hsv.v) * SQUARE_H, 14.))
            .child(self.area_events(Area::Square, cx));
        // Six two-stop gradients make the hue rainbow.
        let segments = (0..6).map(|i| {
            let from = Hsv {
                h: i as f32 * 60.,
                s: 1.,
                v: 1.,
                a: 1.,
            }
            .to_color();
            let to = Hsv {
                h: (i + 1) as f32 * 60.,
                s: 1.,
                v: 1.,
                a: 1.,
            }
            .to_color();
            div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(hsla(from), 0.),
                linear_color_stop(hsla(to), 1.),
            ))
        });
        let hue_strip = div()
            .id("picker-hue")
            .relative()
            .w(px(W))
            .h(px(STRIP_H))
            .cursor(CursorStyle::PointingHand)
            .child(
                div()
                    .flex()
                    .size_full()
                    .rounded(px(STRIP_H / 2.))
                    .overflow_hidden()
                    .children(segments),
            )
            .child(strip_marker(hsv.h / 360.))
            .child(self.area_events(Area::Hue, cx));
        let alpha_strip = div()
            .id("picker-alpha")
            .relative()
            .w(px(W))
            .h(px(STRIP_H))
            .cursor(CursorStyle::PointingHand)
            .child(
                div()
                    .size_full()
                    .rounded(px(STRIP_H / 2.))
                    .border_1()
                    .border_color(c.border_strong)
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(opaque.opacity(0.), 0.),
                        linear_color_stop(opaque, 1.),
                    )),
            )
            .child(strip_marker(hsv.a))
            .child(self.area_events(Area::Alpha, cx));
        let swatch = |color: Hsla| {
            div()
                .size(px(26.))
                .rounded(px(6.))
                .border_1()
                .border_color(c.border_strong)
                .bg(color)
        };
        let focused = picker.hex.focus_handle(cx).is_focused(window);
        let hex_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(swatch(before))
            .child(swatch(now))
            .child(
                input_frame(focused, true, cx)
                    .flex_1()
                    .font_family(mono_font_family())
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .overflow_hidden()
                            .child(picker.hex.clone()),
                    ),
            );
        let label = div()
            .flex()
            .text_size(type_scale(cx).meta)
            .text_color(c.text_muted)
            .font_family(mono_font_family())
            .child(picker.token.path());
        let body = popover(cx)
            .id("color-picker")
            .occlude()
            .p(px(10.))
            .gap(px(10.))
            .track_focus(&picker.focus)
            .on_mouse_down_out(cx.listener(|editor, _, _, cx| editor.close_picker(cx)))
            .on_key_down(cx.listener(|editor, e: &KeyDownEvent, window, cx| {
                if matches!(e.keystroke.key.as_str(), "enter" | "escape") {
                    editor.dismiss_picker(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
            .child(square)
            .child(hue_strip)
            .child(alpha_strip)
            .child(hex_row);
        // Below the swatch when it fits, else above it.
        let fits_below = picker.position.y + px(PICKER_H + 24.) < window.viewport_size().height;
        let anchor = if fits_below {
            anchored()
                .anchor(Corner::TopLeft)
                .position(picker.position + point(px(0.), px(14.)))
        } else {
            anchored()
                .anchor(Corner::BottomLeft)
                .position(picker.position - point(px(0.), px(14.)))
        };
        Some(
            deferred(anchor.snap_to_window_with_margin(px(8.)).child(body))
                .with_priority(1)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_config::Color;

    #[test]
    fn areas_map_the_pointer_to_hsv() {
        let start = Hsv::from_color(Color::rgb(200, 100, 50));
        let top_right = Area::Square.pick(start, 1., 0.);
        assert_eq!((top_right.s, top_right.v), (1., 1.));
        assert_eq!(top_right.h, start.h);
        let bottom = Area::Square.pick(start, 0.3, 1.4);
        assert_eq!(bottom.v, 0.);
        assert_eq!(bottom.to_color(), Color::BLACK);
        let left = Area::Square.pick(start, -2., 0.);
        assert_eq!(left.to_color(), Color::WHITE);
        let hue = Area::Hue.pick(start, 1. / 3., 9.);
        assert!((hue.h - 120.).abs() < 1e-3);
        assert!(Area::Hue.pick(start, 1., 0.).h < 360.);
        let alpha = Area::Alpha.pick(start, 0.5, 0.);
        assert_eq!(alpha.to_color(), Color::rgba(200, 100, 50, 128));
    }

    #[test]
    fn pointer_fractions() {
        let bounds = Bounds::new(point(px(10.), px(20.)), gpui::size(px(100.), px(50.)));
        assert_eq!(fractions(bounds, point(px(60.), px(45.))), (0.5, 0.5));
        assert_eq!(fractions(bounds, point(px(10.), px(20.))), (0., 0.));
    }
}
