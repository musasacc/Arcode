//! The title bar's project dropdown: projects open in other windows (click
//! to bring that window forward), recent projects (click to switch this
//! window, a button to open in a new window, x to forget), and Open
//! Folder / New Project / New Window / Close Project.

use arcode_ui_kit::Elevated;
use std::path::PathBuf;

use arcode_ui_kit::{IconName, Tooltip, icon, kbd, theme};
use gpui::{
    AnyView, App, Context, Div, ElementId, MouseButton, SharedString, Stateful, div, prelude::*, px,
};

use super::Workspace;
use crate::actions::{
    CloseProject, NewProject, NewWindow, OpenFolder, OpenFolderInNewWindow, shortcut_label,
};
use crate::app_state;

/// Most recent projects listed.
const MAX_RECENT: usize = 8;

fn menu_item(
    id: impl Into<ElementId>,
    icon_name: IconName,
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let c = theme(cx).colors.clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(px(30.))
        .px_2()
        .mx_1()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .child(icon(icon_name).size(px(14.)).color(c.text_muted))
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .when(detail.is_none(), |el| el.flex_1())
                .child(label.into()),
        )
        .when_some(detail, |el, detail| {
            el.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(detail),
            )
        })
}

/// A small square button inside a menu row, shown on hover.
fn row_button(
    id: impl Into<ElementId>,
    icon_name: IconName,
    tooltip: &'static str,
    group: SharedString,
    cx: &App,
) -> Stateful<Div> {
    let c = theme(cx).colors.clone();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(20.))
        .rounded_sm()
        .opacity(0.)
        .group_hover(group, |s| s.opacity(1.))
        .hover(|s| s.bg(c.surface))
        .child(icon(icon_name).size(px(12.)).color(c.text_muted))
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip, cx) })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

impl Workspace {
    pub(super) fn render_project_menu(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let current = self.project_root().map(|p| p.to_path_buf());
        let others = crate::windows::open_projects(cx);
        let open_roots: Vec<PathBuf> = others.iter().map(|(r, _)| r.clone()).collect();
        let recent: Vec<(PathBuf, String)> = app_state::recent_projects(cx)
            .iter()
            .filter(|p| Some(&p.path) != current.as_ref() && !open_roots.contains(&p.path))
            .take(MAX_RECENT)
            .map(|p| (p.path.clone(), p.name.clone()))
            .collect();
        let section = |title: &'static str| {
            div()
                .px_3()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(c.text_muted)
                .child(title)
        };
        let separator = || div().h(px(1.)).my_1().bg(c.border);
        let mut menu = div()
            .w(px(360.))
            .py_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if let Some(root) = &current {
            let name = arcode_config::state::project_name(root);
            let detail = SharedString::from(root.display().to_string());
            menu = menu.child(section("Current Window")).child(
                menu_item("current", IconName::FolderOpen, name, Some(detail), cx)
                    .text_color(c.text)
                    .on_click(cx.listener(|ws, _, _, cx| ws.toggle_project_menu(cx))),
            );
        }
        if !others.is_empty() {
            menu = menu.child(section("Open in Other Windows"));
            for (ix, (root, handle)) in others.into_iter().enumerate() {
                let name = arcode_config::state::project_name(&root);
                let detail = SharedString::from(root.display().to_string());
                menu = menu.child(
                    menu_item(
                        ("open-window", ix),
                        IconName::Monitor,
                        name,
                        Some(detail),
                        cx,
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.project_menu_open = false;
                        cx.notify();
                        handle
                            .update(cx, |_, window, _| window.activate_window())
                            .ok();
                    })),
                );
            }
        }
        if current.is_some() || !open_roots.is_empty() {
            menu = menu.child(separator());
        }
        if !recent.is_empty() {
            menu = menu.child(section("Recent Projects"));
            for (ix, (path, name)) in recent.into_iter().enumerate() {
                let detail = SharedString::from(path.display().to_string());
                let group = SharedString::from(format!("menu-recent-{ix}"));
                let (open_path, new_path, remove_path) = (path.clone(), path.clone(), path);
                menu = menu.child(
                    menu_item(("recent", ix), IconName::Folder, name, Some(detail), cx)
                        .group(group.clone())
                        .child(
                            row_button(
                                ("recent-new-window", ix),
                                IconName::ExternalLink,
                                "Open in New Window",
                                group.clone(),
                                cx,
                            )
                            .on_click(cx.listener(
                                move |ws, _, _, cx| {
                                    cx.stop_propagation();
                                    ws.project_menu_open = false;
                                    cx.notify();
                                    crate::windows::open_project_in_new_window(
                                        new_path.clone(),
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            row_button(
                                ("recent-remove", ix),
                                IconName::X,
                                "Remove from Recent Projects",
                                group,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    app_state::remove_recent(&remove_path, cx);
                                    cx.notify();
                                },
                            )),
                        )
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            ws.project_menu_open = false;
                            ws.open_project(open_path.clone(), window, cx);
                        })),
                );
            }
            menu = menu.child(separator());
        }
        let shortcut = |keys: &str| -> SharedString { shortcut_label(keys).into() };
        menu = menu
            .child(
                menu_item(
                    "open-folder",
                    IconName::FolderOpen,
                    "Open Folder…",
                    None,
                    cx,
                )
                .child(kbd(shortcut("secondary-o")))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
            )
            .child(
                menu_item(
                    "open-folder-new-window",
                    IconName::ExternalLink,
                    "Open Folder in New Window…",
                    None,
                    cx,
                )
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(OpenFolderInNewWindow), cx)
                }),
            )
            .child(
                menu_item("new-project", IconName::Plus, "New Project…", None, cx)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewProject), cx)),
            )
            .child(
                menu_item("new-window", IconName::Monitor, "New Window", None, cx)
                    .child(kbd(shortcut("secondary-shift-n")))
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.project_menu_open = false;
                        cx.notify();
                        window.dispatch_action(Box::new(NewWindow), cx);
                    })),
            );
        if self.project.is_some() {
            menu = menu.child(separator()).child(
                menu_item("close-project", IconName::X, "Close Project", None, cx).on_click(
                    cx.listener(|ws, _, window, cx| {
                        // Close the menu first: a confirmation may follow.
                        ws.project_menu_open = false;
                        cx.notify();
                        window.dispatch_action(Box::new(CloseProject), cx);
                    }),
                ),
            );
        }
        menu
    }
}
