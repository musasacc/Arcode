//! The debug console: program output, adapter messages, REPL results, and
//! the evaluate input (Enter evaluates, Up/Down browse the history). Mono
//! 12 on 17 (docs/DESIGN.md 4, 6.8): output muted, what you typed and its
//! results in `text`, errors in `error`.

use arcode_ui_kit::metrics::{CONTROL_H, ICON_BUTTON_RADIUS, type_scale};
use arcode_ui_kit::{IconName, icon, mono_font_family, theme};
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::{DebugPanel, REPL_CONTEXT, ReplHistoryDown, ReplHistoryUp};
use crate::debugger::console::LineKind;

/// Line height of the console.
const LINE_H: f32 = 17.;

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
                    .skip(range.start)
                    .take(range.len())
                    .map(|line| {
                        let color = match line.kind {
                            LineKind::Stdout | LineKind::Info => c.text_muted,
                            LineKind::Stderr | LineKind::Error => c.error,
                            LineKind::Input => c.text,
                            LineKind::Result => c.text,
                        };
                        div()
                            .h(px(LINE_H))
                            .px(px(12.))
                            .whitespace_nowrap()
                            .text_color(color)
                            .child(SharedString::from(if line.text.is_empty() {
                                " ".to_string()
                            } else {
                                line.text.clone()
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.console_scroll.clone())
        .flex_1();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
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
