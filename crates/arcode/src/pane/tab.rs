//! One tab of the pane's tab strip.

use arcode_editor::{DiskConflict, Editor};
use arcode_ui_kit::{IconName, Tooltip, icon, kbd, theme, truncated};
use gpui::{AnyView, Context, Entity, MouseButton, SharedString, div, prelude::*, px};

use super::Pane;
use crate::actions::shortcut_label;
use crate::file_icons::file_icon;

impl Pane {
    pub(super) fn render_tab(
        &self,
        index: usize,
        editor: &Entity<Editor>,
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
        // The dot marks unsaved edits, in the warning color when the file
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
        let group = SharedString::from(format!("tab-{}", editor.entity_id()));
        let close_button = div()
            .id(("close-tab", index))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .hover(|s| s.bg(c.hover))
            .when(!active, |el| {
                el.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .when(dirty, |el| {
                el.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .child(icon(IconName::X).size(px(12.)).color(c.text_muted))
            .on_click(cx.listener(move |pane, _, window, cx| {
                cx.stop_propagation();
                pane.close(index, window, cx);
            }));
        let dirty_dot = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .group_hover(group.clone(), |s| s.opacity(0.))
            .child(div().size(px(8.)).rounded_full().bg(if changed_on_disk {
                c.warning
            } else {
                c.accent_alt
            }));
        div()
            .id(("tab", index))
            .group(group)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .h_full()
            .pl_3()
            .pr_1p5()
            .border_r_1()
            .border_color(c.border)
            .cursor_pointer()
            .bg(if active { c.tab_active } else { c.tab_inactive })
            .text_color(if active { c.text } else { c.text_muted })
            .when(!active, |el| el.hover(|s| s.bg(c.hover)))
            .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip.clone(), cx) })
            .child(icon(icon_name).size(px(14.)).color(icon_color))
            .child(
                div()
                    .max_w(px(220.))
                    .min_w_0()
                    .overflow_hidden()
                    .when(deleted, |el| el.line_through().text_color(c.error))
                    .child(truncated(title)),
            )
            .child(
                div()
                    .relative()
                    .size(px(18.))
                    .flex_none()
                    .when(dirty, |el| el.child(dirty_dot))
                    .child(close_button),
            )
            .when(active, |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .bg(c.accent),
                )
            })
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |pane, _, window, cx| pane.close(index, window, cx)),
            )
            .on_click(cx.listener(move |pane, _, window, cx| pane.activate(index, window, cx)))
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
