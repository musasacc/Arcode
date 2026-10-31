//! One tab of the pane's tab strip.

use arcode_editor::{DiskConflict, Editor};
use arcode_ui_kit::{IconName, kbd, pill_tab, theme};
use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px};

use super::Pane;
use crate::actions::shortcut_label;
use crate::file_icons::file_icon;

impl Pane {
    /// The editor tab at `index`: a pill tab with the ember mark when it
    /// is active in the focused pane, the unsaved ring (in `warning` when
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
        let (icon_name, icon_color) = if custom.is_some() {
            (IconName::GitCompare, c.accent_alt)
        } else {
            file_icon(&title, &c)
        };
        let mut tab = pill_tab(("tab", index), title)
            .icon(icon_name, Some(icon_color))
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
