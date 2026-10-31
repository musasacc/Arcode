//! The call stack: threads and the selected thread's frames. Clicking a
//! thread shows its frames; clicking a frame selects it and opens its line.
//! The current frame is a `pill` row with the ember mark (docs/DESIGN.md
//! 6.8); frames without source are muted.

use arcode_ui_kit::metrics::{ROW_H_DENSE, ROW_INSET, ROW_RADIUS, type_scale};
use arcode_ui_kit::{IconName, ember, icon, mono_font_family, readout, theme};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

use super::DebugPanel;
use crate::debugger::session::frame_location;

impl DebugPanel {
    pub(super) fn render_stack(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let d = self.debugger.read(cx);
        let row = |id: (&'static str, u64)| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(ROW_H_DENSE)
                .mx(ROW_INSET)
                .px(px(6.))
                .rounded(ROW_RADIUS)
                .whitespace_nowrap()
                .cursor_pointer()
        };
        if let Some(s) = d.session() {
            let many = s.threads.len() > 1;
            for thread in &s.threads {
                let selected_thread = s.thread == Some(thread.id);
                if many {
                    let id = thread.id;
                    rows.push(
                        row(("thread", id as u64))
                            .hover(|st| st.bg(c.hover))
                            .text_size(scale.meta)
                            .child(
                                icon(if selected_thread {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(13.))
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
                        row(("frame", frame.id as u64))
                            .pl(px(if many { 22. } else { 6. }))
                            .when(selected, |el| el.bg(c.pill))
                            .when(!selected, |el| el.hover(|st| st.bg(c.hover)))
                            .font_family(mono_font_family())
                            .text_size(scale.readout)
                            .text_color(if selected {
                                c.text
                            } else if subtle {
                                c.text_disabled
                            } else {
                                c.text_muted
                            })
                            .child(
                                div()
                                    .w(px(9.))
                                    .flex_none()
                                    .when(selected, |el| el.child(ember())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(SharedString::from(frame.name.clone())),
                            )
                            .child(div().flex_none().child(readout(file).size(scale.micro)))
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
                        .px(px(12.))
                        .py(px(4.))
                        .text_size(scale.meta)
                        .text_color(c.text_muted)
                        .child(text)
                        .into_any_element(),
                );
            }
        }
        div()
            .id("debug-stack")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .pb(px(6.))
            .overflow_y_scroll()
            .track_scroll(&self.stack_scroll)
            .children(rows)
    }
}
