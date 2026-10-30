//! The branch row and the commit box (message, amend toggle, Commit).

use arcode_git::{CommitOptions, GitError};
use arcode_ui_kit::{IconName, icon, theme, truncated};
use gpui::{Context, Div, Focusable, Window, div, prelude::*, px};

use super::{GitPanel, GitPanelEvent};
use crate::actions::{GitCommit, shortcut_label};
use crate::git::store::GitEvent;

/// The commit message placeholder, naming the shortcut.
pub fn placeholder() -> String {
    format!("Message ({} to commit)", shortcut_label("secondary-enter"))
}

impl GitPanel {
    pub(super) fn render_branch_row(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let branch = store.branch_label().unwrap_or_else(|| "…".into());
        let summary = store.snapshot().summary();
        let merging = store.snapshot().merging;
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .pb_1()
            .child(
                div()
                    .id("git-branch-button")
                    .flex()
                    .items_center()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .px_1p5()
                    .h(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .child(icon(IconName::GitBranch).size(px(14.)).color(c.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(truncated(branch)),
                    )
                    .child(
                        icon(IconName::ChevronDown)
                            .size(px(12.))
                            .color(c.text_muted),
                    )
                    .tooltip(|_, cx| arcode_ui_kit::Tooltip::text("Switch or create a branch", cx))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::PickBranch))),
            )
            .when(merging, |el| {
                el.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(c.warning)
                        .child(icon(IconName::GitMerge).size(px(12.)).color(c.warning))
                        .child("Merging"),
                )
            })
            .when(summary.conflicted > 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(c.error)
                        .child(crate::status_bar::conflict_label(None, summary.conflicted)),
                )
            })
    }

    pub(super) fn render_commit_box(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let rows = self.message.read(cx).visible_rows();
        let line_height = window.line_height();
        let focused = self.message.focus_handle(cx).is_focused(window);
        let staged = self.store.read(cx).snapshot().summary().staged;
        let busy = self.store.read(cx).is_busy();
        let can_commit =
            !busy && (staged > 0 || self.amend || self.store.read(cx).snapshot().merging);
        let label = if self.amend { "Amend" } else { "Commit" };
        let amend = self.amend;
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .px_2()
            .child(
                div()
                    .id("commit-message")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(if focused { c.border_focused } else { c.border })
                    .bg(c.background)
                    .h(line_height * rows as f32 + px(10.))
                    .cursor_text()
                    .child(self.message.clone()),
            )
            .child(
                div().flex().items_center().gap_2().child(
                    div()
                        .id("amend-toggle")
                        .flex()
                        .items_center()
                        .gap_1()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(14.))
                                .rounded_sm()
                                .border_1()
                                .border_color(if amend { c.accent } else { c.border })
                                .when(amend, |el| {
                                    el.bg(c.accent).child(
                                        icon(IconName::Check).size(px(10.)).color(c.accent_text),
                                    )
                                }),
                        )
                        .child("Amend last commit")
                        .on_click(cx.listener(|p, _, _, cx| p.toggle_amend(cx))),
                ),
            )
            .child(
                div()
                    .id("git-commit")
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1p5()
                    .h(px(28.))
                    .rounded_md()
                    .bg(c.accent)
                    .text_color(c.accent_text)
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.accent.opacity(0.85)))
                    .when(!can_commit, |el| el.opacity(0.5))
                    .child(icon(IconName::Check).size(px(14.)).color(c.accent_text))
                    .child(label)
                    .tooltip(move |_, cx| {
                        let text = format!("{label} ({})", shortcut_label("secondary-enter"));
                        arcode_ui_kit::Tooltip::text(text, cx)
                    })
                    .on_click(cx.listener(|p, _, window, cx| p.commit(&GitCommit, window, cx))),
            )
    }

    fn toggle_amend(&mut self, cx: &mut Context<Self>) {
        self.amend = !self.amend;
        cx.notify();
        if !self.amend || !self.message.read(cx).text().trim().is_empty() {
            return;
        }
        let message = self.message.clone();
        self.store.update(cx, |s, cx| {
            s.run(
                arcode_git::last_commit_message,
                move |_, last, cx| {
                    if let Some(last) = last {
                        message.update(cx, |m, cx| {
                            if m.text().is_empty() {
                                m.set_text(&last, cx);
                            }
                        });
                    }
                },
                cx,
            )
        });
    }

    /// Commits the staged changes with the message (or amends).
    pub fn commit(&mut self, _: &GitCommit, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.message.read(cx).text().to_string();
        if text.trim().is_empty() {
            self.store.update(cx, |_, cx| {
                cx.emit(GitEvent::Message(GitError::EmptyMessage.to_string()))
            });
            return;
        }
        let options = CommitOptions { amend: self.amend };
        let panel = cx.entity().downgrade();
        self.store.update(cx, |s, cx| {
            s.run(
                move |repo| arcode_git::commit_with(repo, &text, options),
                move |_, sha, cx| {
                    cx.emit(GitEvent::Message(format!("Committed {sha}")));
                    panel
                        .update(cx, |p, cx| {
                            p.amend = false;
                            p.message.update(cx, |m, cx| m.set_text("", cx));
                        })
                        .ok();
                },
                cx,
            )
        });
    }
}
