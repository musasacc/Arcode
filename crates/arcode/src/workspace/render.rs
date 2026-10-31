//! Layout of the workspace (docs/DESIGN.md 6.1): the canvas with the
//! title bar, the icon rail and the footer drawn on it, and the sidebar,
//! editor and bottom panels floating on it with 6 px gaps that hold the
//! resize handles; plus divider dragging.

use arcode_ui_kit::metrics::GAP;
use arcode_ui_kit::{panel, panel_border, settings, theme};
use gpui::{
    App, Context, CursorStyle, Div, MouseButton, MouseMoveEvent, Window, div, prelude::*, px,
};

use super::anim::Slider;
use super::frame::{self, FramedPanel};
use super::{Resize, Workspace};
use crate::activity_bar::{self, Activity, RailState};
use crate::bottom_panel;
use crate::prefs::tour::TourTarget;
use crate::sidebar;
use crate::status_bar;
use crate::{title_bar, welcome};

/// Drawn last inside a panel: canvas-coloured slivers outside its rounded
/// corners and the 1 px edge again on top. GPUI clips children to a
/// square, so a child that fills the panel (a view background, a row band)
/// would otherwise square off the 10 px corners and cover the edge's arcs.
/// Two quads, no listeners (clicks pass through).
pub(crate) fn corner_caps(focused: bool, cx: &App) -> Div {
    let c = &theme(cx).colors;
    let radius = arcode_ui_kit::metrics::PANEL_RADIUS;
    // Absolute children sit in the padding box, 1 px inside the edge.
    let edge = px(-1.);
    let ring = px(6.);
    div()
        .absolute()
        .inset_0()
        .child(arcode_ui_kit::border_width(
            div()
                .absolute()
                .top(edge - ring)
                .left(edge - ring)
                .right(edge - ring)
                .bottom(edge - ring)
                .rounded(radius + ring)
                .border_color(c.canvas),
            ring,
        ))
        .child(
            div()
                .absolute()
                .top(edge)
                .left(edge)
                .right(edge)
                .bottom(edge)
                .rounded(radius)
                .border_1()
                .border_color(panel_border(focused, cx)),
        )
}

/// A resize handle filling the gap it sits in.
fn handle(id: &'static str, cursor: CursorStyle) -> gpui::Stateful<Div> {
    div().id(id).absolute().cursor(cursor)
}

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
                self.sidebar_width = frame::sidebar_width_at(
                    event.position.x,
                    px(sidebar::MIN_WIDTH),
                    px(sidebar::MAX_WIDTH),
                );
            }
            Resize::BottomPanel => {
                self.bottom_panel_height = frame::bottom_height_at(
                    event.position.y,
                    window.viewport_size().height,
                    px(bottom_panel::MIN_HEIGHT),
                );
            }
        }
        cx.notify();
    }

    fn render_sidebar(&self, framed: bool, cx: &mut Context<Self>) -> Div {
        let content = match (self.activity, &self.file_tree) {
            (Activity::Files, Some(tree)) => tree.clone().into_any_element(),
            (Activity::Files, None) => sidebar::no_folder(cx).into_any_element(),
            (Activity::Git, _) => self.git.panel.clone().into_any_element(),
            (Activity::Search, _) => self.features.search.clone().into_any_element(),
            (Activity::Todo, _) => self.features.todo_side.clone().into_any_element(),
            (Activity::Debug, _) => self.debug.sidebar.clone().into_any_element(),
            #[cfg(feature = "plugins")]
            (Activity::Plugins, _) => self.plugins_page_element(),
            #[cfg(not(feature = "plugins"))]
            (Activity::Plugins, _) => self.langs.view.clone().into_any_element(),
            (Activity::Packages, _) => self.pkgs.view.clone().into_any_element(),
            #[cfg(feature = "plugins")]
            (Activity::Extension, _) => self.render_plugin_sidebar(cx),
            #[cfg(not(feature = "plugins"))]
            (Activity::Extension, _) => div().into_any_element(),
        };
        // The panel plus the gap to its right, which holds the handle; both
        // slide together.
        div()
            .relative()
            .flex()
            .flex_none()
            .w(frame::sidebar_slot(self.sidebar_width))
            .h_full()
            .pr(GAP)
            .child(
                panel(framed, cx)
                    .relative()
                    .track_focus(&self.shell.sidebar_focus)
                    .flex_1()
                    .h_full()
                    .child(content)
                    .child(corner_caps(framed, cx)),
            )
            .child(
                handle("sidebar-resize", CursorStyle::ResizeLeftRight)
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(frame::HANDLE)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, window, cx| {
                            // Keep keyboard focus where it is while dragging.
                            window.prevent_default();
                            ws.start_resize(Resize::Sidebar, cx)
                        }),
                    ),
            )
    }

    fn render_bottom_panel(&self, framed: bool, cx: &mut Context<Self>) -> Div {
        self.bottom_panel
            .update(cx, |panel, _| panel.set_framed(framed));
        // The gap above the panel holds the handle and slides with it.
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .h(frame::bottom_slot(self.bottom_panel_height))
            .pt(GAP)
            .child(
                panel(framed, cx)
                    .relative()
                    .track_focus(&self.shell.bottom_focus)
                    .flex_1()
                    .w_full()
                    .child(self.bottom_panel.clone())
                    .child(self.prefs.anchors.recorder(TourTarget::TerminalPanel))
                    .child(corner_caps(framed, cx)),
            )
            .child(
                handle("panel-resize", CursorStyle::ResizeUpDown)
                    .left_0()
                    .right_0()
                    .top_0()
                    .h(frame::HANDLE)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, window, cx| {
                            window.prevent_default();
                            ws.start_resize(Resize::BottomPanel, cx)
                        }),
                    ),
            )
    }

    /// The editor panel: every split pane lives in it, divided by 1 px
    /// hairlines (no accent line over the active pane: the ember mark on
    /// its active tab shows where you are).
    fn render_editor_panel(&self, framed: bool, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let mut row = div().flex().flex_1().min_h_0().w_full();
        for (ix, pane) in self.panes.iter().enumerate() {
            if ix > 0 {
                row = row.child(div().w(px(1.)).h_full().flex_none().bg(c.border));
            }
            // The ember mark goes on the framed panel's active pane.
            let ember = framed && ix == self.active_pane;
            pane.update(cx, |pane, _| pane.set_framed(ember));
            row = row.child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(pane.clone()),
            );
        }
        panel(framed, cx)
            .relative()
            .track_focus(&self.shell.editor_focus)
            .flex_1()
            .min_h_0()
            .w_full()
            .children(self.render_package_banner(cx))
            .child(row)
            .child(corner_caps(framed, cx))
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        status_bar::mark_ready(cx);
        crate::prefs::frames::tick();
        let c = theme(cx).colors.clone();
        let ui = settings(cx).ui.clone();
        let welcome = self.shows_welcome(cx);
        let body = if welcome {
            // One unframed panel fills the body, 6 px of canvas at the sides.
            div().flex().flex_1().min_h_0().px(GAP).child(
                panel(false, cx)
                    .relative()
                    .flex_1()
                    .child(welcome::render(self, cx))
                    .child(corner_caps(false, cx)),
            )
        } else {
            let activity = self.activity;
            #[cfg(feature = "plugins")]
            let extras = {
                let this = cx.entity().downgrade();
                activity_bar::render_extras(
                    self.plugin_activities(),
                    std::rc::Rc::new(move |ix, window: &mut Window, cx: &mut gpui::App| {
                        this.update(cx, |ws, cx| ws.plugin_sidebar_clicked(ix, window, cx))
                            .ok();
                    }),
                    cx,
                )
            };
            #[cfg(not(feature = "plugins"))]
            let extras = Vec::new();
            let rail = RailState {
                active: activity,
                sidebar_visible: self.sidebar_visible,
                bottom_visible: self.bottom_panel_visible,
                git_changes: self.git_change_count(cx),
                todos: self.features.todo.read(cx).count(),
            };
            let bar = activity_bar::render(
                rail,
                {
                    let this = cx.entity().downgrade();
                    std::rc::Rc::new(move |activity, _: &mut Window, cx: &mut gpui::App| {
                        this.update(cx, |ws, cx| ws.select_activity(activity, cx))
                            .ok();
                    })
                },
                extras,
                cx,
            );
            let bar = div()
                .relative()
                .flex_none()
                .h_full()
                .child(bar)
                .child(self.prefs.anchors.recorder(TourTarget::ActivityBar));
            let show_sidebar = self.sidebar_visible || self.sliding_closed(Slider::Sidebar);
            let show_panel = self.bottom_panel_visible || self.sliding_closed(Slider::Panel);
            // Focus moves the frame only on focus events (they redraw);
            // nothing animates it.
            let framed = self.shell.update_frame(
                self.sidebar_visible,
                self.bottom_panel_visible,
                &self.focus_handle,
                window,
                cx,
            );
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .pr(GAP)
                .child(bar)
                .when(show_sidebar, |el| {
                    let sidebar = self.render_sidebar(framed == FramedPanel::Sidebar, cx);
                    let slot = frame::sidebar_slot(self.sidebar_width);
                    el.child(self.slide_wrap(Slider::Sidebar, sidebar, slot, cx))
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_editor_panel(framed == FramedPanel::Editor, cx))
                        .when(show_panel, |el| {
                            let panel = self.render_bottom_panel(framed == FramedPanel::Bottom, cx);
                            let slot = frame::bottom_slot(self.bottom_panel_height);
                            el.child(self.slide_wrap(Slider::Panel, panel, slot, cx))
                        }),
                )
        };
        let root = div()
            .id("workspace")
            // While the first-run dialog or the tour is up, no workspace
            // shortcut may act behind it.
            .key_context(if self.modal_open() {
                crate::actions::MODAL_CONTEXT
            } else {
                crate::actions::WORKSPACE_CONTEXT
            })
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.canvas)
            .text_color(c.text)
            .text_size(px(ui.font_size))
            .font_family(arcode_ui_kit::ui_font_family(cx))
            .when(self.resizing.is_some(), |el| {
                el.cursor(match self.resizing {
                    Some(Resize::BottomPanel) => CursorStyle::ResizeUpDown,
                    _ => CursorStyle::ResizeLeftRight,
                })
            })
            .on_mouse_move(cx.listener(Self::on_mouse_move))
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
            .child(
                div()
                    .relative()
                    .flex_none()
                    .map(|el| {
                        if welcome {
                            let memory = self.status_bar.read(cx).memory_mib();
                            el.child(status_bar::welcome_footer(memory, cx))
                        } else {
                            el.child(self.status_bar.clone())
                        }
                    })
                    .child(self.prefs.anchors.recorder(TourTarget::Footer)),
            );
        let root = self.chrome_listeners(root, welcome, cx);
        let root = self.register_actions(root, cx);
        let root = self.register_git_actions(root, cx);
        let root = self.register_nav_actions(root, cx);
        let root = self.register_feature_actions(root, cx);
        let root = self.register_file_actions(root, cx);
        let root = self.register_disk_actions(root, cx);
        let root = self.register_debug_actions(root, cx);
        let root = self.register_language_actions(root, cx);
        let root = self.register_prefs_actions(root, cx);
        let root = self.register_packages_actions(root, cx);
        #[cfg(feature = "plugins")]
        let root = self.register_plugin_actions(root, cx);
        let root = self.render_overlays(root, cx);
        let root = self.render_run_overlays(root, cx);
        let root = self.render_new_project_overlay(root, cx);
        let root = self.render_palette_overlay(root, cx);
        let root = self.render_nav_overlay(root, cx);
        let root = self.render_prefs_overlays(root, cx);
        #[cfg(feature = "plugins")]
        let root = self.render_plugin_overlay(root, cx);
        self.render_tour_overlay(root, window, cx)
    }
}
