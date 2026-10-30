//! The states without a file list: no folder, loading, not a repository,
//! and a status that could not be read.

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, theme};
use gpui::{App, Context, Div, SharedString, div, prelude::*, px};

use super::{GitPanel, rows};

impl GitPanel {
    pub(super) fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        if store.root().is_none() {
            return rows::note("No folder is open.", cx).into_any_element();
        }
        if !store.is_loaded() {
            return rows::note("Loading…", cx).into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .px_4()
            .py_6()
            .text_sm()
            .text_color(c.text_muted)
            .child(
                icon(IconName::GitBranch)
                    .size(px(32.))
                    .color(c.text_disabled),
            )
            .child(
                div()
                    .text_center()
                    .child("This folder is not a git repository."),
            )
            .child(
                button("git-init", "Initialize Repository")
                    .style(ButtonStyle::Primary)
                    .on_click(
                        cx.listener(|p, _, _, cx| {
                            p.store.update(cx, |s, cx| s.init_repository(cx))
                        }),
                    ),
            )
            .into_any_element()
    }
}

/// Why the file list is empty although there may be changes (git missing,
/// a broken repository): shown instead of "No changes.".
pub(super) fn error_note(text: impl Into<SharedString>, cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .items_start()
        .gap_1p5()
        .px_3()
        .py_2()
        .text_sm()
        .text_color(c.error)
        .child(
            div()
                .flex_none()
                .mt(px(2.))
                .child(icon(IconName::TriangleAlert).size(px(14.)).color(c.error)),
        )
        .child(div().flex_1().min_w_0().child(text.into()))
}
