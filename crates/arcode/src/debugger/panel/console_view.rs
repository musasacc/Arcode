//! The debug console: program output, adapter messages, REPL results, and
//! the evaluate input (Enter evaluates, Up/Down browse the history). Mono
//! 12 on 17 (docs/DESIGN.md 4, 6.8): output muted, what you typed and its
//! results in `text`, errors in `error`.
//!
//! The list is as tall as the whole lines that fit, so the first line
//! never shows cut in half when the console follows its end; long lines
//! end with an ellipsis (the whole line is in a tooltip).

use arcode_ui_kit::metrics::{CONTROL_H, ICON_BUTTON_RADIUS, type_scale};
use arcode_ui_kit::{IconName, Tooltip, icon, mono_font_family, theme};
use gpui::{Context, IntoElement, Pixels, SharedString, canvas, div, prelude::*, px, uniform_list};

use super::{DebugPanel, REPL_CONTEXT, ReplHistoryDown, ReplHistoryUp};
use crate::debugger::console::LineKind;

/// Line height of the console.
const LINE_H: f32 = 17.;

/// Lines longer than this (chars) get a tooltip with the whole line, as
/// they may end in an ellipsis.
const TOOLTIP_CHARS: usize = 32;

/// The height of the whole `line_h` lines that fit in `available`.
pub(super) fn whole_lines(available: Pixels, line_h: Pixels) -> Pixels {
    if line_h <= Pixels::ZERO {
        return available;
    }
    line_h * (available / line_h).floor().max(0.)
}

impl DebugPanel {
    pub(super) fn render_console(
        &self,
        repl_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let count = self.debugger.read(cx).console().len();
        let list = uniform_list(
            "debug-console",
            count,
            cx.processor(move |panel, range: std::ops::Range<usize>, _, cx| {
                let c = theme(cx).colors.clone();
                let console = panel.debugger.read(cx).console();
                console
                    .lines()
                    .enumerate()
                    .skip(range.start)
                    .take(range.len())
                    .map(|(ix, line)| {
                        let color = match line.kind {
                            LineKind::Stdout | LineKind::Info => c.text_muted,
                            LineKind::Stderr | LineKind::Error => c.error,
                            LineKind::Input => c.text,
                            LineKind::Result => c.text,
                        };
                        let text = SharedString::from(if line.text.is_empty() {
                            " ".to_string()
                        } else {
                            line.text.clone()
                        });
                        let long = text.chars().count() > TOOLTIP_CHARS;
                        div()
                            .id(("console-line", ix))
                            .h(px(LINE_H))
                            .w_full()
                            .px(px(12.))
                            .truncate()
                            .text_color(color)
                            .when(long, |el| {
                                let tip = text.clone();
                                el.tooltip(move |_, cx| Tooltip::text(tip.clone(), cx))
                            })
                            .child(text)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.console_scroll.clone());
        // Whole lines only: the list takes the height of the lines that
        // fit, and what is left stays empty under it.
        let list = match self.console_h {
            Some(h) => list.flex_none().h(whole_lines(h, px(LINE_H))),
            None => list.flex_1(),
        };
        let panel = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                panel
                    .update(cx, |panel, cx| {
                        if panel.console_h != Some(bounds.size.height) {
                            panel.console_h = Some(bounds.size.height);
                            cx.notify();
                        }
                    })
                    .ok();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(measure)
                    .font_family(mono_font_family())
                    .text_size(scale.readout)
                    .line_height(px(LINE_H))
                    .child(list),
            )
            .child(
                div()
                    .key_context(REPL_CONTEXT)
                    .on_action(cx.listener(Self::submit_repl))
                    .on_action(cx.listener(|p, _: &ReplHistoryUp, _, cx| p.history(true, cx)))
                    .on_action(cx.listener(|p, _: &ReplHistoryDown, _, cx| p.history(false, cx)))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(CONTROL_H)
                    .mx(px(8.))
                    .mb(px(8.))
                    .mt(px(4.))
                    .px(px(8.))
                    .rounded(ICON_BUTTON_RADIUS)
                    .border_1()
                    .border_color(if repl_focused {
                        c.accent
                    } else {
                        c.border_strong
                    })
                    .text_size(scale.body)
                    .line_height(px(20.))
                    .child(icon(IconName::ChevronRight).size(px(13.)).color(c.accent))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(CONTROL_H - px(2.))
                            .child(self.repl.clone()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_shows_whole_lines() {
        assert_eq!(whole_lines(px(100.), px(17.)), px(85.));
        assert_eq!(whole_lines(px(102.), px(17.)), px(102.));
        assert_eq!(whole_lines(px(16.), px(17.)), px(0.));
        assert_eq!(whole_lines(px(0.), px(17.)), px(0.));
        assert_eq!(whole_lines(px(50.), px(0.)), px(50.));
    }
}
