//! The debug console: program output, adapter messages, REPL results, and
//! the input line (Enter evaluates, Up/Down browse the history).

use arcode_ui_kit::{IconName, icon, settings, theme};
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::{DebugPanel, REPL_CONTEXT, ReplHistoryDown, ReplHistoryUp};
use crate::debugger::console::LineKind;

impl DebugPanel {
    pub(super) fn render_console(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let mono = settings(cx).editor.font_family.clone();
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
                            LineKind::Stdout => c.text,
                            LineKind::Stderr | LineKind::Error => c.error,
                            LineKind::Info => c.text_muted,
                            LineKind::Input => c.accent,
                            LineKind::Result => c.info,
                        };
                        div()
                            .h(px(18.))
                            .px_2()
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
            .bg(c.background)
            .text_xs()
            .font_family(mono)
            .child(list)
            .child(
                div()
                    .key_context(REPL_CONTEXT)
                    .on_action(cx.listener(Self::submit_repl))
                    .on_action(cx.listener(|p, _: &ReplHistoryUp, _, cx| p.history(true, cx)))
                    .on_action(cx.listener(|p, _: &ReplHistoryDown, _, cx| p.history(false, cx)))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(26.))
                    .px_2()
                    .border_t_1()
                    .border_color(c.border)
                    .child(icon(IconName::ChevronRight).size(px(12.)).color(c.accent))
                    .child(div().flex_1().min_w_0().h_full().child(self.repl.clone())),
            )
    }
}
