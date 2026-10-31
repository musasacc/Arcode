//! The call stack: threads and the selected thread's frames. Clicking a
//! thread shows its frames; clicking a frame selects it and opens its line.

use arcode_ui_kit::{IconName, icon, theme};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

use super::DebugPanel;
use crate::debugger::session::frame_location;

impl DebugPanel {
    pub(super) fn render_stack(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        let d = self.debugger.read(cx);
        if let Some(s) = d.session() {
            let many = s.threads.len() > 1;
            for thread in &s.threads {
                let selected_thread = s.thread == Some(thread.id);
                if many {
                    let id = thread.id;
                    rows.push(
                        div()
                            .id(("thread", id as u64))
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(22.))
                            .px_2()
                            .cursor_pointer()
                            .hover(|st| st.bg(c.hover))
                            .child(
                                icon(if selected_thread {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(12.))
                                .color(c.text_muted),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .child(SharedString::from(thread.name.clone())),
                            )
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                panel
                                    .debugger
                                    .update(cx, |d, cx| d.load_frames(id, false, cx));
                            }))
                            .into_any_element(),
                    );
                }
                if !selected_thread {
                    continue;
                }
                for frame in &s.frames {
                    let selected = s.frame == Some(frame.id);
                    let location = frame_location(frame);
                    let file: SharedString = location
                        .as_ref()
                        .and_then(|(p, line)| {
                            Some(format!("{}:{}", p.file_name()?.to_string_lossy(), line + 1))
                        })
                        .unwrap_or_default()
                        .into();
                    let subtle =
                        location.is_none() || frame.presentation_hint.as_deref() == Some("subtle");
                    let target = frame.clone();
                    rows.push(
                        div()
                            .id(("frame", frame.id as u64))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(22.))
                            .pl(px(if many { 22. } else { 8. }))
                            .pr_2()
                            .cursor_pointer()
                            .when(selected, |el| el.bg(c.selection))
                            .hover(|st| st.bg(c.hover))
                            .text_color(if subtle { c.text_muted } else { c.text })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(SharedString::from(frame.name.clone())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(c.text_muted)
                                    .child(file),
                            )
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                panel
                                    .debugger
                                    .update(cx, |d, cx| d.select_frame(&target, true, cx));
                            }))
                            .into_any_element(),
                    );
                }
            }
            if rows.is_empty() {
                let text = if s.is_stopped() {
                    "Loading…"
                } else if s.is_terminated() {
                    "The session ended."
                } else {
                    "Running. Pause (F6) or hit a breakpoint to see the call stack."
                };
                rows.push(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(text)
                        .into_any_element(),
                );
            }
        }
        div()
            .id("debug-stack")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.stack_scroll)
            .text_sm()
            .children(rows)
    }
}
