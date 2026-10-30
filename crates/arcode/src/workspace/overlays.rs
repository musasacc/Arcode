//! Floating layers above the workspace: the project menu, the file finder
//! and toast messages. Each fades in when animations are enabled.

use std::path::PathBuf;

use arcode_ui_kit::{IconName, animation_duration, icon, kbd, theme};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Div, ElementId, MouseButton, SharedString,
    Stateful, Window, div, ease_out_quint, prelude::*, px,
};

use super::Workspace;
use crate::actions::{CloseProject, NewProject, OpenFolder, shortcut_label};
use crate::app_state;
use crate::title_bar::TITLE_BAR_HEIGHT;

/// Wraps `el` in a short fade-in, or returns it unchanged when animations
/// are off.
pub fn fade_in(id: impl Into<ElementId>, el: Div, cx: &App) -> AnyElement {
    match animation_duration(cx) {
        Some(duration) => el
            .with_animation(
                id,
                Animation::new(duration).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
        None => el.into_any_element(),
    }
}

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

impl Workspace {
    fn render_project_menu(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let current = self.project.as_ref().map(|p| p.root.clone());
        let recent: Vec<(PathBuf, String)> = app_state::recent_projects(cx)
            .iter()
            .filter(|p| Some(&p.path) != current.as_ref())
            .take(8)
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
            .w(px(320.))
            .py_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .shadow_md()
            .text_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !recent.is_empty() {
            menu = menu.child(section("Recent Projects"));
            for (ix, (path, name)) in recent.into_iter().enumerate() {
                let detail = SharedString::from(path.display().to_string());
                menu = menu.child(
                    menu_item(("recent", ix), IconName::Folder, name, Some(detail), cx).on_click(
                        cx.listener(move |ws, _, window, cx| {
                            ws.project_menu_open = false;
                            ws.open_project(path.clone(), window, cx);
                        }),
                    ),
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
                menu_item("new-project", IconName::Plus, "New Project…", None, cx)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewProject), cx)),
            );
        if self.project.is_some() {
            menu = menu.child(separator()).child(
                menu_item("close-project", IconName::X, "Close Project", None, cx)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CloseProject), cx)),
            );
        }
        menu
    }

    fn backdrop(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
        on_close: fn(&mut Self, &mut Window, &mut Context<Self>),
    ) -> Stateful<Div> {
        div().id(id).absolute().inset_0().occlude().on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _, window, cx| on_close(ws, window, cx)),
        )
    }

    /// Adds the visible overlays as the last children of `root`.
    pub(super) fn render_overlays(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let mut root = root;
        if self.project_menu_open {
            let menu = self.render_project_menu(cx);
            root = root.child(
                self.backdrop("menu-backdrop", cx, |ws, _, cx| {
                    ws.project_menu_open = false;
                    cx.notify();
                })
                .child(
                    div()
                        .absolute()
                        .top(px(TITLE_BAR_HEIGHT - 2.))
                        .left(px(8.))
                        .child(fade_in("project-menu-anim", menu, cx)),
                ),
            );
        }
        if let Some(finder) = &self.finder {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(finder.clone());
            root = root.child(
                self.backdrop("finder-backdrop", cx, |ws, window, cx| {
                    ws.close_finder(window, cx)
                })
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + 8.))
                .child(fade_in("finder-anim", panel, cx)),
            );
        }
        if let Some(picker) = &self.branch_picker {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(picker.clone());
            root = root.child(
                self.backdrop("branch-backdrop", cx, |ws, window, cx| {
                    ws.close_branch_picker(window, cx)
                })
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + 8.))
                .child(fade_in("branch-anim", panel, cx)),
            );
        }
        if let Some(prompt) = &self.path_prompt {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(prompt.clone());
            root = root.child(
                self.backdrop("path-backdrop", cx, |ws, window, cx| {
                    ws.path_prompt = None;
                    ws.path_prompt_then = None;
                    ws.active_pane().read(cx).focus(window, cx);
                    cx.notify();
                })
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + 8.))
                .child(fade_in("path-anim", panel, cx)),
            );
        }
        if let Some(toast) = &self.toast {
            let toast_el = div()
                .max_w(px(420.))
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(c.border)
                .bg(c.surface_raised)
                .shadow_md()
                .text_sm()
                .child(icon(IconName::Info).size(px(14.)).color(c.info))
                .child(div().flex_1().min_w_0().child(toast.clone()));
            root = root.child(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(36.))
                    .child(fade_in("toast-anim", toast_el, cx)),
            );
        }
        root
    }
}
