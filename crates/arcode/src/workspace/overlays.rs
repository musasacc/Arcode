//! Floating layers above the workspace: the project menu, the file finder,
//! toast messages and the discard-changes prompt. Each fades in when animations are enabled.

use arcode_ui_kit::Elevated;
use arcode_ui_kit::{IconName, animation_duration, icon, theme};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Div, ElementId, MouseButton, Stateful,
    Window, div, ease_out_quint, prelude::*, px,
};

use super::Workspace;
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

impl Workspace {
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
        if let Some((picker, _)) = &self.langs.picker {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(picker.clone());
            root = root.child(
                self.backdrop("list-picker-backdrop", cx, |ws, window, cx| {
                    ws.close_list_picker(window, cx)
                })
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + 8.))
                .child(fade_in("list-picker-anim", panel, cx)),
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
                .elevated(cx)
                .text_sm()
                .child(icon(IconName::Info).size(px(14.)).color(c.info))
                .child(div().flex_1().min_w_0().child(toast.clone()));
            root = root.child(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(36.))
                    .child(fade_in(("toast-anim", self.toast_seq), toast_el, cx)),
            );
        }
        root
    }
}
