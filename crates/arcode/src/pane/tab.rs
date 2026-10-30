//! One tab of the pane's tab strip.

use arcode_editor::Editor;
use arcode_ui_kit::{IconName, Tooltip, icon, theme, truncated};
use gpui::{AnyView, Context, Entity, MouseButton, SharedString, div, prelude::*, px};

use super::Pane;
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
        let dirty = e.is_dirty() && custom.is_none();
        let tooltip: SharedString = match &custom {
            Some(label) => label.tooltip.clone(),
            None => e
                .path()
                .map(|p| p.display().to_string().into())
                .unwrap_or_else(|| title.clone()),
        };
        let active = index == self.active;
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
            .child(div().size(px(8.)).rounded_full().bg(c.accent_alt));
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
}
