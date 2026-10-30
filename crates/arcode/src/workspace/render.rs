//! Layout of the workspace: title bar, activity bar, sidebar, panes,
//! bottom panel and status bar, plus divider dragging.

use arcode_ui_kit::{settings, theme};
use gpui::{Context, CursorStyle, Div, MouseButton, MouseMoveEvent, Window, div, prelude::*, px};

use super::{Resize, Workspace};
use crate::activity_bar::{self, Activity};
use crate::bottom_panel;
use crate::sidebar;
use crate::status_bar;
use crate::{title_bar, welcome};

/// Width of the invisible drag handle on dividers.
const HANDLE: f32 = 6.;

impl Workspace {
    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(resizing) = self.resizing else {
            return;
        };
        if !event.dragging() {
            self.resizing = None;
            cx.notify();
            return;
        }
        match resizing {
            Resize::Sidebar => {
                let x = event.position.x - px(activity_bar::ACTIVITY_BAR_WIDTH);
                self.sidebar_width = x.clamp(px(sidebar::MIN_WIDTH), px(sidebar::MAX_WIDTH));
            }
            Resize::BottomPanel => {
                let bottom = window.viewport_size().height - px(24.);
                let max = (window.viewport_size().height * 0.75).max(px(bottom_panel::MIN_HEIGHT));
                self.bottom_panel_height =
                    (bottom - event.position.y).clamp(px(bottom_panel::MIN_HEIGHT), max);
            }
        }
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let content = match (self.activity, &self.file_tree) {
            (Activity::Files, Some(tree)) => tree.clone().into_any_element(),
            (Activity::Files, None) => sidebar::no_folder(cx).into_any_element(),
            (other, _) => sidebar::placeholder(other, cx).into_any_element(),
        };
        div()
            .relative()
            .flex_none()
            .w(self.sidebar_width)
            .h_full()
            .bg(c.surface)
            .border_r_1()
            .border_color(c.border)
            .child(content)
            .child(
                div()
                    .id("sidebar-resize")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(-HANDLE / 2.))
                    .w(px(HANDLE))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, _, cx| ws.start_resize(Resize::Sidebar, cx)),
                    ),
            )
    }

    fn render_bottom_panel(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        div()
            .relative()
            .flex_none()
            .h(self.bottom_panel_height)
            .border_t_1()
            .border_color(c.border)
            .child(self.bottom_panel.clone())
            .child(
                div()
                    .id("panel-resize")
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(-HANDLE / 2.))
                    .h(px(HANDLE))
                    .cursor(CursorStyle::ResizeUpDown)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, _, cx| ws.start_resize(Resize::BottomPanel, cx)),
                    ),
            )
    }

    fn render_panes(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let many = self.panes.len() > 1;
        let mut row = div().flex().flex_1().min_h_0().w_full();
        for (ix, pane) in self.panes.iter().enumerate() {
            if ix > 0 {
                row = row.child(div().w(px(1.)).h_full().flex_none().bg(c.border));
            }
            let active = ix == self.active_pane;
            row = row.child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(pane.clone())
                    .when(many && active, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(1.))
                                .bg(c.accent),
                        )
                    }),
            );
        }
        row
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        status_bar::mark_ready(cx);
        let c = theme(cx).colors.clone();
        let ui = settings(cx).ui.clone();
        let welcome = self.shows_welcome(cx);
        let body = if welcome {
            div().flex_1().min_h_0().child(welcome::render(self, cx))
        } else {
            let activity = self.activity;
            let bar = activity_bar::render(
                activity,
                self.sidebar_visible,
                {
                    let this = cx.entity().downgrade();
                    std::rc::Rc::new(move |activity, _: &mut Window, cx: &mut gpui::App| {
                        this.update(cx, |ws, cx| ws.select_activity(activity, cx))
                            .ok();
                    })
                },
                cx,
            );
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(bar)
                .when(self.sidebar_visible, |el| el.child(self.render_sidebar(cx)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_panes(cx))
                        .when(self.bottom_panel_visible, |el| {
                            el.child(self.render_bottom_panel(cx))
                        }),
                )
        };
        let root = div()
            .id("workspace")
            .key_context(crate::actions::WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.background)
            .text_color(c.text)
            .text_size(px(ui.font_size))
            .when_some(ui.font_family.clone(), |el, family| el.font_family(family))
            .when(self.resizing.is_some(), |el| {
                el.cursor(match self.resizing {
                    Some(Resize::BottomPanel) => CursorStyle::ResizeUpDown,
                    _ => CursorStyle::ResizeLeftRight,
                })
            })
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_key_down(cx.listener(|ws, event: &gpui::KeyDownEvent, _, cx| {
                if ws.project_menu_open && event.keystroke.key == "escape" {
                    ws.project_menu_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|ws, _, _, cx| {
                    if ws.resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(title_bar::render(self, window, cx))
            .child(body)
            .child(self.status_bar.clone());
        let root = self.register_actions(root, cx);
        self.render_overlays(root, cx)
    }
}
