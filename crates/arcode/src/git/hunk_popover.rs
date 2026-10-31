//! The small popover opened by clicking a change bar: the old text of the
//! hunk and Stage / Unstage / Revert buttons.

use arcode_git::{ChangeKind, GutterHunk};
use arcode_ui_kit::Elevated;
use arcode_ui_kit::{ButtonStyle, IconName, button, icon_button, settings, theme};
use gpui::{
    Context, EntityId, IntoElement, MouseButton, Render, WeakEntity, Window, div, prelude::*, px,
};

use super::editor::{GUTTER_SET, GitEditors, HunkAction};

/// Most old lines shown before eliding the rest.
const MAX_PREVIEW_LINES: usize = 12;

/// Popover content for one hunk.
pub struct HunkPopover {
    hunk: GutterHunk,
    editor: EntityId,
    index: usize,
    git: WeakEntity<GitEditors>,
}

impl HunkPopover {
    /// A popover for hunk `index` of `editor`.
    pub fn new(
        hunk: GutterHunk,
        editor: EntityId,
        index: usize,
        git: WeakEntity<GitEditors>,
    ) -> Self {
        Self {
            hunk,
            editor,
            index,
            git,
        }
    }

    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: HunkAction,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let git = self.git.clone();
        let (editor, index) = (self.editor, self.index);
        let style = if action == HunkAction::Revert {
            ButtonStyle::Ghost
        } else {
            ButtonStyle::Secondary
        };
        let c = theme(cx).colors.clone();
        div()
            .when(!enabled, |el| el.opacity(0.4))
            .child(button(id, label).style(style).on_click(move |_, _, cx| {
                if enabled {
                    git.update(cx, |git, cx| git.hunk_action(editor, index, action, cx))
                        .ok();
                }
            }))
            .text_color(c.text)
    }
}

/// `Modified · 3 lines` and similar.
pub fn hunk_title(hunk: &GutterHunk) -> String {
    let (kind, count) = match hunk.kind {
        ChangeKind::Added => ("Added", hunk.hunk.new_lines),
        ChangeKind::Modified => ("Modified", hunk.hunk.new_lines),
        ChangeKind::Deleted => ("Deleted", hunk.hunk.old_lines),
    };
    let plural = if count == 1 { "" } else { "s" };
    let staged = if hunk.staged { " · staged" } else { "" };
    format!("{kind} · {count} line{plural}{staged}")
}

impl Render for HunkPopover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let editor_font = settings(cx).editor.font_family.clone();
        let old_lines: Vec<String> = self
            .hunk
            .hunk
            .old_text
            .lines()
            .map(|l| l.replace('\t', "    "))
            .collect();
        let hidden = old_lines.len().saturating_sub(MAX_PREVIEW_LINES);
        let preview = div()
            .flex()
            .flex_col()
            .max_w(px(640.))
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(c.border)
            .font_family(editor_font)
            .text_xs()
            .children(old_lines.into_iter().take(MAX_PREVIEW_LINES).map(|line| {
                div()
                    .px_2()
                    .bg(c.diff_deleted.opacity(0.15))
                    .whitespace_nowrap()
                    .child(format!("- {line}"))
            }))
            .when(hidden > 0, |el| {
                el.child(
                    div()
                        .px_2()
                        .text_color(c.text_muted)
                        .child(format!("… {hidden} more lines")),
                )
            });
        let staged = self.hunk.staged;
        let has_old = !self.hunk.hunk.old_text.is_empty();
        let git = self.git.clone();
        let editor = self.editor;
        div()
            .id("hunk-popover")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .min_w(px(280.))
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .text_color(c.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(c.text_muted)
                            .child(hunk_title(&self.hunk)),
                    )
                    .child(
                        icon_button("hunk-close", IconName::X)
                            .size(px(20.))
                            .tooltip("Close (Esc)")
                            .on_click(move |_, _, cx| {
                                git.update(cx, |git, cx| {
                                    if let Some(e) =
                                        git.editors.get(&editor).and_then(|s| s.editor.upgrade())
                                    {
                                        e.update(cx, |e, cx| e.hide_popover(GUTTER_SET, cx));
                                    }
                                })
                                .ok();
                            }),
                    ),
            )
            .when(has_old, |el| el.child(preview))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(self.action_button(
                        "hunk-stage",
                        "Stage hunk",
                        HunkAction::Stage,
                        !staged,
                        cx,
                    ))
                    .child(self.action_button(
                        "hunk-unstage",
                        "Unstage hunk",
                        HunkAction::Unstage,
                        staged,
                        cx,
                    ))
                    .child(self.action_button(
                        "hunk-revert",
                        "Revert hunk",
                        HunkAction::Revert,
                        true,
                        cx,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_describe_the_change() {
        let hunks = arcode_git::gutter_hunks(&arcode_git::diff_hunks("a\nb\n", "a\nB\nc\n"), &[]);
        assert_eq!(hunk_title(&hunks[0]), "Modified · 2 lines");
        let staged = arcode_git::gutter_hunks(&[], &arcode_git::diff_hunks("a\n", ""));
        assert_eq!(hunk_title(&staged[0]), "Deleted · 1 line · staged");
    }
}
