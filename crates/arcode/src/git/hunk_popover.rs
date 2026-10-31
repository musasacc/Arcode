//! The small popover opened by clicking a change bar: the old text of the
//! hunk and Stage / Unstage / Revert buttons.

use arcode_git::{ChangeKind, GutterHunk};
use arcode_ui_kit::{
    ButtonStyle, IconName, button, icon_button, popover, settings, theme, type_scale,
    ui_font_family,
};
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
    ) -> impl IntoElement + use<> {
        let git = self.git.clone();
        let (editor, index) = (self.editor, self.index);
        let style = if action == HunkAction::Revert {
            ButtonStyle::Ghost
        } else {
            ButtonStyle::Secondary
        };
        button(id, label)
            .style(style)
            .compact()
            .disabled(!enabled)
            .on_click(move |_, _, cx| {
                git.update(cx, |git, cx| git.hunk_action(editor, index, action, cx))
                    .ok();
            })
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
        let scale = type_scale(cx);
        let editor_font = settings(cx).editor.font_family.clone();
        let old_lines: Vec<String> = self
            .hunk
            .hunk
            .old_text
            .lines()
            .map(|l| l.replace('\t', "    "))
            .collect();
        let hidden = old_lines.len().saturating_sub(MAX_PREVIEW_LINES);
        // The old lines on the editor's `panel` fill, each marked with a
        // deleted `−` (no alpha tints, docs/DESIGN.md 2).
        let preview = div()
            .flex()
            .flex_col()
            .max_w(px(640.))
            .py(px(4.))
            .overflow_hidden()
            .rounded(px(6.))
            .border_1()
            .border_color(c.border)
            .bg(c.panel())
            .font_family(editor_font)
            .text_size(scale.meta)
            .children(old_lines.into_iter().take(MAX_PREVIEW_LINES).map(|line| {
                div()
                    .flex()
                    .px(px(8.))
                    .whitespace_nowrap()
                    .child(div().flex_none().w(px(14.)).text_color(c.error).child("−"))
                    .child(line)
            }))
            .when(hidden > 0, |el| {
                el.child(
                    div()
                        .px(px(8.))
                        .text_color(c.text_muted)
                        .child(format!("… {hidden} more lines")),
                )
            });
        let staged = self.hunk.staged;
        let has_old = !self.hunk.hunk.old_text.is_empty();
        let git = self.git.clone();
        let editor = self.editor;
        popover(cx)
            .id("hunk-popover")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .gap(px(8.))
            .p(px(8.))
            .min_w(px(280.))
            .font_family(ui_font_family(cx))
            .text_size(scale.body)
            .text_color(c.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(hunk_title(&self.hunk)),
                    )
                    .child(
                        icon_button("hunk-close", IconName::X)
                            .size(px(22.))
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
                    .gap(px(6.))
                    .child(self.action_button(
                        "hunk-stage",
                        "Stage hunk",
                        HunkAction::Stage,
                        !staged,
                    ))
                    .child(self.action_button(
                        "hunk-unstage",
                        "Unstage hunk",
                        HunkAction::Unstage,
                        staged,
                    ))
                    .child(self.action_button(
                        "hunk-revert",
                        "Revert hunk",
                        HunkAction::Revert,
                        true,
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
