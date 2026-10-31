//! The bar above an editor whose file changed on disk while it had unsaved
//! edits: Reload (discard my changes), Compare, Keep Mine. It does not
//! block typing; the workspace handles its actions.

use arcode_editor::{DiskConflict, Editor};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, theme, truncated};
use gpui::{Action, AnyElement, App, Entity, Focusable, Hsla, Rgba, actions, div, prelude::*, px};

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
                    .flex()
                    .min_w_0()
                    .overflow_hidden()
                    .child(truncated("This file changed on disk.")),
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
                .bg(tint(c.panel(), c.warning, 0.12))
                .border_b_1()
                .border_color(c.border)
                .text_sm()
                .child(message)
                .child(buttons)
                .into_any_element(),
        )
    }
}

/// `color` laid over `base` at `amount`, as one opaque colour: the bar's
/// warm wash without an alpha layer (docs/DESIGN.md 5.2).
fn tint(base: Hsla, color: Hsla, amount: f32) -> Hsla {
    let (a, b) = (Rgba::from(base), Rgba::from(color));
    let mix = |x: f32, y: f32| x + (y - x) * amount;
    Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: 1.,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_fill_is_opaque() {
        let base: Hsla = Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        }
        .into();
        let warm: Hsla = Rgba {
            r: 1.,
            g: 0.5,
            b: 0.,
            a: 1.,
        }
        .into();
        let fill = Rgba::from(tint(base, warm, 0.5));
        assert_eq!(fill.a, 1.);
        assert!((fill.r - 0.5).abs() < 0.01 && (fill.g - 0.25).abs() < 0.01);
        assert_eq!(Rgba::from(tint(base, warm, 0.)).r, 0.);
    }
}
