//! Drawing the Clone Repository dialog (docs/DESIGN.md 6.12, 6.13).

use arcode_ui_kit::metrics::WEIGHT_SEMIBOLD;
use arcode_ui_kit::{
    ButtonStyle, IconName, button, icon, input_frame, mono_font_family, theme, type_scale,
};
use gpui::{
    App, Context, Entity, Focusable, IntoElement, SharedString, Window, div, prelude::*, px,
};

use super::{CloneDialog, Dismiss, StartClone};
use crate::picker_frame::dialog_frame;
use crate::text_field::TextField;

/// Width of the dialog.
const WIDTH: f32 = 520.;

impl CloneDialog {
    /// The progress block while a clone runs: what git is doing, how far,
    /// and a thin bar.
    fn render_progress(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let running = self.running.as_ref()?;
        let c = theme(cx).colors.clone();
        let meta = type_scale(cx).meta;
        let (phase, percent) = match &running.progress {
            Some(p) => (SharedString::from(p.phase.clone()), p.percent),
            None => ("Connecting…".into(), None),
        };
        let fill = percent.map_or(0., |p| f32::from(p) / 100.);
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(meta)
                        .text_color(c.text_muted)
                        .child(phase)
                        .children(
                            percent.map(|p| {
                                div().font_family(mono_font_family()).child(format!("{p}%"))
                            }),
                        ),
                )
                .child(
                    div()
                        .id("clone-progress")
                        .h(px(4.))
                        .w_full()
                        .rounded(px(2.))
                        .bg(c.pill)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .bg(c.accent)
                                .w(gpui::relative(fill)),
                        ),
                ),
        )
    }
}

impl Render for CloneDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let meta = type_scale(cx).meta;
        let field = |label: &'static str, input: Entity<TextField>, cx: &App| {
            let focused = input.focus_handle(cx).is_focused(window);
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(meta).text_color(c.text_muted).child(label))
                .child(
                    input_frame(focused, true, cx)
                        .min_w_0()
                        .overflow_hidden()
                        .child(input),
                )
        };
        let running = self.running.is_some();
        // Worked out from the fields alone: no disk access while drawing.
        let target: Option<SharedString> = {
            let folder = self.folder.read(cx).text().trim();
            let location = self.location.read(cx).text().trim();
            (!folder.is_empty() && !location.is_empty()).then(|| {
                let path = crate::path_prompt::expand_home(location).join(folder);
                let home = std::env::home_dir();
                let shown = crate::welcome::tilde_path(&path, home.as_deref());
                format!("Clones into {shown}").into()
            })
        };
        let terminal_style = if self.terminal_hint {
            ButtonStyle::Secondary
        } else {
            ButtonStyle::Ghost
        };
        dialog_frame(px(WIDTH), cx)
            .key_context(super::CONTEXT)
            .on_action(cx.listener(Self::start))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::next_field))
            .on_action(cx.listener(Self::previous_field))
            .gap(px(8.))
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(4.))
                    .text_size(type_scale(cx).title)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .child(icon(IconName::GitBranch).size(px(16.)).color(c.accent))
                    .child("Clone repository"),
            )
            .child(field("Repository URL", self.url.clone(), cx))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(field(
                        "Location",
                        self.location.clone(),
                        cx,
                    )))
                    .child(
                        button("clone-browse", "Browse…")
                            .disabled(running)
                            .on_click(cx.listener(|d, _, window, cx| d.browse(window, cx))),
                    ),
            )
            .child(field("Folder name", self.folder.clone(), cx))
            .children(target.filter(|_| self.error.is_none()).map(|text| {
                div()
                    .text_size(meta)
                    .text_color(c.text_muted)
                    .overflow_hidden()
                    .child(arcode_ui_kit::truncated(text).middle())
            }))
            .children(self.render_progress(cx))
            .children(self.error.clone().map(|message| {
                let color = if self.notice_only {
                    c.text_muted
                } else {
                    c.error
                };
                div().text_size(meta).text_color(color).child(message)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt(px(4.))
                    .child(
                        button("clone-terminal", "Clone in Terminal")
                            .icon(IconName::Terminal)
                            .style(terminal_style)
                            .disabled(running)
                            .tooltip(
                                "Run git clone in the terminal, where it can ask for credentials",
                            )
                            .on_click(cx.listener(|d, _, _, cx| d.clone_in_terminal(cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                button("clone-cancel", "Cancel")
                                    .style(if running {
                                        ButtonStyle::Secondary
                                    } else {
                                        ButtonStyle::Ghost
                                    })
                                    .on_click(cx.listener(|d, _, window, cx| {
                                        d.cancel(&Dismiss, window, cx)
                                    })),
                            )
                            .child(
                                button("clone-start", if running { "Cloning…" } else { "Clone" })
                                    .style(ButtonStyle::Primary)
                                    .disabled(running)
                                    .on_click(cx.listener(|d, _, window, cx| {
                                        d.start(&StartClone, window, cx)
                                    })),
                            ),
                    ),
            )
    }
}
