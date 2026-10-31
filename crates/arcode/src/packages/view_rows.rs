//! Rendering of the Packages list rows: manifest headers with their
//! toolbar, dependencies with hover buttons, notes.

use arcode_project::deps::{DepKind, PackageAction};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, icon_button, theme};
use gpui::{
    AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, SharedString, div, prelude::*, px,
};

use super::badges::{badge, dep_tooltip, manager_color};
use super::rows::{Row, relative_dir, version_text};
use super::store::CheckState;
use super::view::{PackagesView, ROW_HEIGHT};
use super::view_actions::{DepCommand, can_add, removable};

impl PackagesView {
    pub(super) fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.rows.get(ix) {
            Some(Row::Section(section)) => self.render_section(ix, *section, cx),
            Some(Row::Dep { section, dep }) => self.render_dep(ix, *section, *dep, cx),
            Some(Row::Note(section)) => self.render_note(*section, cx),
            None => div().into_any_element(),
        }
    }

    fn render_section(&self, ix: usize, section: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let Some(m) = store.manifests().get(section) else {
            return div().into_any_element();
        };
        let collapsed = self.collapsed.contains(&m.path);
        let rel = relative_dir(store.root(), &m.path);
        let file = m.file_name();
        let title = m.name.clone().unwrap_or_else(|| {
            if rel.is_empty() {
                file.clone()
            } else {
                rel.clone()
            }
        });
        let mut location = if rel.is_empty() {
            file.clone()
        } else {
            format!("{rel}/{file}")
        };
        if m.workspace_root {
            location = format!("workspace · {location}");
        }
        let path = m.path.clone();
        let addable = can_add(m);
        let check = store.check(&m.path).cloned();
        let running = check == Some(CheckState::Running);
        let tool = |id: &'static str, glyph: IconName, tip: &'static str| {
            icon_button((id, ix), glyph).size(px(20.)).tooltip(tip)
        };
        let (p1, p2, p3, p4, p5) = (path.clone(), path.clone(), path.clone(), path.clone(), path);
        let status = match &check {
            Some(CheckState::Running) => Some(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(icon(IconName::Loader).size(px(12.)).color(c.accent))
                    .child("checking")
                    .child(
                        tool("pkg-check-cancel", IconName::X, "Cancel the check").on_click(
                            cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                view.store.update(cx, |s, cx| s.cancel_check(&p4, cx));
                            }),
                        ),
                    )
                    .into_any_element(),
            ),
            Some(CheckState::Failed(msg)) => Some(
                div()
                    .id(("pkg-check-failed", ix))
                    .flex_none()
                    .child(icon(IconName::CircleAlert).size(px(12.)).color(c.error))
                    .tooltip({
                        let msg = SharedString::from(msg.clone());
                        move |_, cx| arcode_ui_kit::Tooltip::text(msg.clone(), cx)
                    })
                    .into_any_element(),
            ),
            Some(CheckState::Done(_))
                if !m.deps.iter().any(|d| self.newer(&m.path, d, cx).is_some()) =>
            {
                Some(
                    div()
                        .id(("pkg-check-ok", ix))
                        .flex_none()
                        .child(icon(IconName::CircleCheck).size(px(12.)).color(c.success))
                        .tooltip(|_, cx| {
                            arcode_ui_kit::Tooltip::text("No newer versions found", cx)
                        })
                        .into_any_element(),
                )
            }
            _ => None,
        };
        div()
            .id(("pkg-section", ix))
            .group("pkg-section")
            .relative()
            .flex()
            .items_center()
            .gap_1()
            .h(px(ROW_HEIGHT))
            .w_full()
            .pl_1()
            .pr_1()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .bg(c.surface_raised.opacity(0.5))
            .hover(|s| s.bg(c.hover))
            .child(
                icon(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(px(12.))
                .color(c.text_muted),
            )
            .child(badge(m.manager.label(), manager_color(m.manager, &c)))
            .child(
                div()
                    .flex_shrink()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(location),
            )
            .children(status)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(4.))
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .pl_1()
                    .bg(c.surface)
                    .invisible()
                    .group_hover("pkg-section", |s| s.visible())
                    .when(addable, |el| {
                        el.child(tool("pkg-add", IconName::Plus, "Add a package…").on_click(
                            cx.listener(move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.open_add(&p1, window, cx);
                            }),
                        ))
                    })
                    .child(
                        tool("pkg-update-all", IconName::ArrowUp, "Update all").on_click(
                            cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                view.run(&p2, PackageAction::UpdateAll, cx);
                            }),
                        ),
                    )
                    .when(running, |el| {
                        el.child(
                            tool("pkg-check-stop", IconName::X, "Cancel the check").on_click(
                                cx.listener(move |view, _, _, cx| {
                                    cx.stop_propagation();
                                    view.store.update(cx, |s, cx| s.cancel_check(&p5, cx));
                                }),
                            ),
                        )
                    })
                    .when(!running, |el| {
                        el.child(
                            tool(
                                "pkg-check",
                                IconName::Search,
                                "Check for updates (uses the network)",
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    cx.stop_propagation();
                                    view.store.update(cx, |s, cx| s.check_updates(&p3, cx));
                                },
                            )),
                        )
                    })
                    .child(tool("pkg-refresh", IconName::Refresh, "Refresh").on_click(
                        cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.store.update(cx, |s, cx| s.refresh(cx));
                        }),
                    )),
            )
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_section(section, cx)))
            .into_any_element()
    }

    fn render_dep(
        &self,
        ix: usize,
        section: usize,
        dep: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let Some(m) = store.manifests().get(section) else {
            return div().into_any_element();
        };
        let Some(d) = m.deps.get(dep) else {
            return div().into_any_element();
        };
        let confirming = self
            .confirm_remove
            .as_ref()
            .is_some_and(|key| key.matches(&m.path, d));
        let base = div()
            .id(("pkg-dep", ix))
            .group("pkg-row")
            .relative()
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(ROW_HEIGHT))
            .w_full()
            .pl(px(24.))
            .pr_2()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|s| s.bg(c.hover));
        if confirming {
            return base
                .bg(c.error.opacity(0.12))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(format!("Remove {}?", d.name)),
                )
                .child(
                    button(("pkg-remove-yes", ix), "Remove")
                        .style(ButtonStyle::Primary)
                        .on_click(cx.listener(|view, _, _, cx| view.confirm_removal(cx))),
                )
                .child(
                    icon_button(("pkg-remove-no", ix), IconName::X)
                        .size(px(20.))
                        .tooltip("Keep it")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.confirm_remove = None;
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let newer = self.newer(&m.path, d, cx);
        let detail = dep_tooltip(d);
        let has_docs = m.manager.docs_url(&d.package).is_some();
        let can_remove = removable(m, d);
        let action = |id: &'static str, glyph: IconName, tip: &'static str, kind: DepCommand| {
            icon_button((id, ix), glyph)
                .size(px(20.))
                .tooltip(tip)
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.dep_action(section, dep, kind, cx);
                }))
        };
        let actions = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(4.))
            .flex()
            .items_center()
            .gap_0p5()
            .pl_1()
            .bg(c.surface)
            .invisible()
            .group_hover("pkg-row", |s| s.visible())
            .child(action(
                "pkg-dep-update",
                IconName::ArrowUp,
                "Update",
                DepCommand::Update,
            ))
            .when(can_remove, |el| {
                el.child(action(
                    "pkg-dep-remove",
                    IconName::Trash,
                    "Remove…",
                    DepCommand::Remove,
                ))
            })
            .when(has_docs, |el| {
                el.child(action(
                    "pkg-dep-docs",
                    IconName::ExternalLink,
                    "Open documentation",
                    DepCommand::Docs,
                ))
            });
        let kind_color = match d.kind {
            DepKind::Dev | DepKind::Build => c.info,
            DepKind::Optional | DepKind::Peer => c.warning,
            _ => c.text_muted,
        };
        base.child(div().flex_none().text_color(c.text).child(d.name.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(version_text(d)),
            )
            .when(d.kind != DepKind::Normal, |el| {
                el.child(badge(d.kind.label(), kind_color))
            })
            .when(!d.source.label().is_empty(), |el| {
                el.child(badge(d.source.label(), c.text_muted))
            })
            .when_some(newer, |el, latest| {
                el.child(badge(format!("newer: {latest}"), c.success))
            })
            .child(actions)
            .when(self.menu.is_none(), |el| {
                el.tooltip(move |_, cx| arcode_ui_kit::Tooltip::text(detail.clone(), cx))
            })
            .on_click(cx.listener(move |view, _, _, cx| view.open_dep(section, dep, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.open_menu(section, dep, event.position, cx)
                }),
            )
            .into_any_element()
    }

    fn render_note(&self, section: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let (text, color) = match store.manifests().get(section) {
            Some(m) => match &m.error {
                Some(err) => (err.clone(), c.error),
                None if m.deps.is_empty() => ("No dependencies".to_string(), c.text_muted),
                None => ("No packages match the filter".to_string(), c.text_muted),
            },
            None => (String::new(), c.text_muted),
        };
        div()
            .flex()
            .items_center()
            .h(px(ROW_HEIGHT))
            .pl(px(24.))
            .pr_2()
            .w_full()
            .overflow_hidden()
            .text_xs()
            .text_color(color)
            .child(div().truncate().child(text))
            .into_any_element()
    }
}
