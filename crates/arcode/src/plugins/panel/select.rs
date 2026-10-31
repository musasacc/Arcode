//! The `select` widget: a drop-down of plain-text options. The choice is
//! shown at once and sent as `select(i)` (a gesture); the plugin's next
//! render of the node replaces the local choice.

use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::{EventKind, SelectProps};
use arcode_ui_kit::{IconName, icon, menu_row, popover, theme};
use gpui::{AnyElement, Context, SharedString, anchored, deferred, div, prelude::*, px};

use super::PanelView;

impl PanelView {
    pub(super) fn render_select(
        &mut self,
        id: NodeId,
        p: &SelectProps,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let selected = self.selects.get(&id).copied().or(p.selected);
        let label = selected
            .and_then(|i| p.options.get(i as usize))
            .cloned()
            .unwrap_or_else(|| "Select…".to_owned());
        let open = self.open_select == Some(id);
        let trigger = div()
            .id(("select", id))
            .flex()
            .items_center()
            .gap_1()
            .h(px(28.))
            .px_2()
            .max_w(px(320.))
            .rounded(px(8.))
            .border_1()
            .border_color(if open { c.accent } else { c.border_strong })
            .bg(c.panel())
            .cursor_pointer()
            .hover(|s| s.bg(c.hover))
            .child(div().min_w_0().truncate().child(label))
            .child(
                icon(IconName::ChevronDown)
                    .size(px(12.))
                    .color(c.text_muted),
            )
            .on_click(cx.listener(move |view, _, _, cx| {
                view.open_select = if view.open_select == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }));
        let menu = open.then(|| {
            let options = p.options.iter().enumerate().map(|(i, option)| {
                menu_row(("option", i), selected == Some(i as u32), cx)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(option.clone())),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| {
                        let i = i as u32;
                        view.open_select = None;
                        view.selects.insert(id, i);
                        cx.notify();
                        view.send(id, EventKind::Select(i), cx);
                    }))
            });
            let options: Vec<_> = options.collect();
            deferred(
                anchored().snap_to_window().child(
                    popover(cx)
                        .id(("select-menu", id))
                        .mt_1()
                        .min_w(px(160.))
                        .max_w(px(480.))
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .children(options)
                        .on_mouse_down_out(cx.listener(move |view, _, _, cx| {
                            if view.open_select == Some(id) {
                                view.open_select = None;
                                cx.notify();
                            }
                        })),
                ),
            )
            .with_priority(1)
        });
        div()
            .flex_none()
            .child(trigger)
            .children(menu)
            .into_any_element()
    }
}
