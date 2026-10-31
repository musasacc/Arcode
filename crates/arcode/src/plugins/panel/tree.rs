//! The `tree` widget: a flat, virtualized list of items with `depth`
//! (docs/PLUGINS.md §7). The chevron sends `tree-toggle` (no gesture) and
//! shows a spinner until the plugin patches the children in; clicking an
//! item selects it and sends `tree-activate` (a gesture).

use std::rc::Rc;

use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::{EventKind, TreeItem};
use arcode_ui_kit::{IconName, icon, theme, truncated, type_scale};
use gpui::{AnyElement, Context, MouseButton, SharedString, div, prelude::*, px, uniform_list};

use super::PanelView;
use crate::plugins::commands::icon_for;

/// Height of a tree row.
const TREE_ROW: f32 = 24.;
/// Indentation per level.
const INDENT: f32 = 14.;

impl PanelView {
    fn tree_toggle(&mut self, id: NodeId, key: String, cx: &mut Context<Self>) {
        self.tree_pending.insert((id, key.clone()));
        cx.notify();
        self.send(id, EventKind::TreeToggle(key), cx);
    }

    fn tree_activate(&mut self, id: NodeId, key: String, cx: &mut Context<Self>) {
        self.tree_selected.insert(id, key.clone());
        cx.notify();
        self.send(id, EventKind::TreeActivate(key), cx);
    }

    pub(super) fn render_tree(
        &mut self,
        id: NodeId,
        items: &[TreeItem],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let meta = type_scale(cx).meta;
        let items: Rc<[TreeItem]> = items.into();
        let selected = self.tree_selected.get(&id).cloned();
        let pending: Vec<String> = self
            .tree_pending
            .iter()
            .filter(|(n, _)| *n == id)
            .map(|(_, k)| k.clone())
            .collect();
        let weak = cx.entity().downgrade();
        let count = items.len();
        uniform_list(("tree", id), count, move |range, _, _| {
            let Some(view) = weak.upgrade() else {
                return Vec::new();
            };
            range
                .filter_map(|ix| {
                    let item = items.get(ix)?.clone();
                    let waiting = pending.contains(&item.key);
                    let is_selected = selected.as_deref() == Some(item.key.as_str());
                    let chevron = if waiting {
                        Some(IconName::Loader)
                    } else if item.expandable && item.expanded {
                        Some(IconName::ChevronDown)
                    } else if item.expandable {
                        Some(IconName::ChevronRight)
                    } else {
                        None
                    };
                    let (toggle_view, toggle_key) = (view.clone(), item.key.clone());
                    let (open_view, open_key) = (view.clone(), item.key.clone());
                    let expandable = item.expandable;
                    Some(
                        div()
                            .id(("tree-row", ix))
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(TREE_ROW))
                            .pl(px(4. + INDENT * f32::from(item.depth)))
                            .pr_2()
                            .cursor_pointer()
                            .when(is_selected, |el| el.bg(c.selection))
                            .hover(|s| s.bg(c.hover))
                            .child(
                                div()
                                    .id(("tree-chevron", ix))
                                    .size(px(16.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when_some(chevron, |el, name| {
                                        el.child(icon(name).size(px(12.)).color(c.text_muted))
                                    })
                                    .when(expandable, |el| {
                                        el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            cx.stop_propagation();
                                            let key = toggle_key.clone();
                                            toggle_view
                                                .update(cx, |v, cx| v.tree_toggle(id, key, cx));
                                        })
                                    }),
                            )
                            .when_some(item.icon.as_deref(), |el, name| {
                                el.child(icon(icon_for(Some(name))).size(px(14.)).color(c.accent))
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(truncated(item.label.clone())),
                            )
                            .when_some(item.detail.clone(), |el, detail| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .text_size(meta)
                                        .text_color(c.text_muted)
                                        .child(SharedString::from(detail)),
                                )
                            })
                            .on_click(move |_, _, cx| {
                                let key = open_key.clone();
                                open_view.update(cx, |v, cx| v.tree_activate(id, key, cx));
                            })
                            .into_any_element(),
                    )
                })
                .collect()
        })
        .size_full()
        .into_any_element()
    }
}
