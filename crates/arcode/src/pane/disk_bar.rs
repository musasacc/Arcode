//! The bar above an editor whose file changed on disk while it had unsaved
//! edits: Reload (discard my changes), Compare, Keep Mine. It does not
//! block typing; the workspace handles its actions.

use arcode_editor::{DiskConflict, Editor};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, theme};
use gpui::{Action, AnyElement, App, Entity, Focusable, actions, div, prelude::*, px};

use super::Pane;

actions!(
    file,
    [
        /// Replaces the active editor's text with the file on disk
        /// (unsaved changes are discarded; undo brings them back).
        ReloadFromDisk,
        /// Opens a diff of the file on disk against the active editor.
        CompareWithDisk,
        /// Keeps the active editor's text over the changed file; the next
        /// save writes it without asking.
        KeepMyChanges,
    ]
);

impl Pane {
    /// The bar for the active editor, when its file changed on disk.
    pub(super) fn render_disk_bar(&self, editor: &Entity<Editor>, cx: &App) -> Option<AnyElement> {
        if editor.read(cx).disk_conflict() != Some(DiskConflict::Changed) {
            return None;
        }
        let c = theme(cx).colors.clone();
        // The actions go to the editor (focused first), so the workspace
        // acts on this pane's editor.
        let send = |action: &'static dyn Action, editor: &Entity<Editor>| {
            let editor = editor.clone();
            move |_: &gpui::ClickEvent, window: &mut gpui::Window, cx: &mut App| {
                let handle = editor.focus_handle(cx);
                window.focus(&handle);
                handle.dispatch_action(action, window, cx);
            }
        };
        // The message and the buttons wrap onto more lines in a narrow
        // pane, so every choice stays visible and clickable.
        let message = div()
            .debug_selector(|| "disk-conflict-message".into())
            .flex()
            .flex_1()
            .min_w(px(160.))
            .items_center()
            .gap_2()
            .child(icon(IconName::TriangleAlert).size(px(16.)).color(c.warning))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child("This file changed on disk."),
            );
        let buttons = div()
            .debug_selector(|| "disk-conflict-buttons".into())
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                button("disk-reload", "Reload (discard my changes)")
                    .style(ButtonStyle::Secondary)
                    .tooltip("Replace your text with the file on disk (undo brings it back)")
                    .on_click(send(&ReloadFromDisk, editor)),
            )
            .child(
                button("disk-compare", "Compare")
                    .style(ButtonStyle::Secondary)
                    .tooltip("Show the file on disk against your version")
                    .on_click(send(&CompareWithDisk, editor)),
            )
            .child(
                button("disk-keep", "Keep mine")
                    .style(ButtonStyle::Primary)
                    .tooltip("Keep your version; saving overwrites the file")
                    .on_click(send(&KeepMyChanges, editor)),
            );
        Some(
            div()
                .id("disk-conflict-bar")
                .debug_selector(|| "disk-conflict-bar".into())
                .flex()
                .flex_wrap()
                .flex_none()
                .items_center()
                .gap_2()
                .px_3()
                .py(px(5.))
                .bg(c.warning.opacity(0.12))
                .border_b_1()
                .border_color(c.border)
                .text_sm()
                .child(message)
                .child(buttons)
                .into_any_element(),
        )
    }
}
