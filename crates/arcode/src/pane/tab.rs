//! One tab of the pane's tab strip.

use arcode_editor::{DiskConflict, Editor};
use arcode_ui_kit::{FileGlyph, IconName, kbd, pill_tab, settings, theme};
use gpui::{App, Context, Entity, Hsla, MouseButton, SharedString, div, prelude::*, px};

use super::Pane;
use crate::actions::shortcut_label;
use crate::file_icons::file_icon_parts;

impl Pane {
    /// The icon of the tab of `editor` titled `title`, and its tint: a
    /// diff tab's `git-compare`, else the file's icon
    /// ([`crate::file_icons`]), by its path when it has one.
    pub(crate) fn tab_glyph(
        &self,
        editor: &Entity<Editor>,
        title: &str,
        cx: &App,
    ) -> (FileGlyph, Hsla) {
        let c = &theme(cx).colors;
        if self.titles.contains_key(&editor.entity_id()) {
            return (FileGlyph::Icon(IconName::GitCompare), c.accent_alt);
        }
        let e = editor.read(cx);
        let path = e.path().unwrap_or(std::path::Path::new(title));
        file_icon_parts(path, settings(cx), c)
    }

    /// The editor tab at `index`: a pill tab (filled while it is active in
    /// the focused pane, quieter in other panes), the unsaved ring (in `warning` when
    /// the file also changed on disk), and the name struck
    /// through when the file was deleted on disk.
    pub(super) fn render_tab(
        &self,
        index: usize,
        editor: &Entity<Editor>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let custom = self.titles.get(&editor.entity_id()).cloned();
        let e = editor.read(cx);
        let title = custom
            .as_ref()
            .map_or_else(|| e.title(), |t| t.title.clone());
        let conflict = e.disk_conflict().filter(|_| custom.is_none());
        let changed_on_disk = conflict == Some(DiskConflict::Changed);
        let deleted = conflict == Some(DiskConflict::Deleted);
        // The ring marks unsaved edits, in the warning color when the file
        // also changed on disk.
        let dirty = (e.is_dirty() || changed_on_disk) && custom.is_none();
        let tooltip: SharedString = match &custom {
            Some(label) => label.tooltip.clone(),
            None => {
                let path = e
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| title.to_string());
                match conflict {
                    Some(DiskConflict::Changed) => format!("{path}\nChanged on disk").into(),
                    Some(DiskConflict::Deleted) => format!("{path}\nDeleted on disk").into(),
                    None => path.into(),
                }
            }
        };
        let active = index == self.active && self.active_page.is_none();
        let (glyph, tint) = self.tab_glyph(editor, &title, cx);
        let mut tab = pill_tab(("tab", index), title)
            .file_glyph(glyph, Some(tint))
            .active(active)
            .focused(focused)
            .dirty(dirty)
            .tooltip(tooltip)
            .on_click(cx.listener(move |pane, _, window, cx| pane.activate(index, window, cx)))
            .on_close(cx.listener(move |pane, _, window, cx| pane.close(index, window, cx)));
        if changed_on_disk {
            tab = tab.ring_color(c.warning);
        }
        if deleted {
            tab = tab.strikethrough(true).label_color(c.error);
        }
        tab.build(cx).on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |pane, _, window, cx| pane.close(index, window, cx)),
        )
    }

    pub(super) fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let hint = |label: &'static str, keys: &str| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .w(px(240.))
                .child(label)
                .child(kbd(shortcut_label(keys)))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .text_sm()
            .text_color(c.text_muted)
            .child(hint("Find file", "secondary-p"))
            .child(hint("Split editor", "secondary-\\"))
            .child(hint("Toggle sidebar", "secondary-b"))
            .child(hint("Toggle panel", "secondary-j"))
    }
}
