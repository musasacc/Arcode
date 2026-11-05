//! The welcome screen in the workspace: its body, the Clone Repository
//! dialog and Clear Recent Projects.

use std::path::PathBuf;

use arcode_ui_kit::metrics::GAP;
use arcode_ui_kit::panel;
use gpui::{
    AppContext, Context, Div, Entity, Focusable, MouseButton, Stateful, Window, div, prelude::*,
};

use super::Workspace;
use super::anim::Slider;
use super::frame;
use super::overlays::fade_in;
use crate::actions::{ClearRecentProjects, CloneRepository};
use crate::app_state;
use crate::clone_dialog::{CloneDialog, CloneEvent, CloneRequest, terminal_command};
use crate::welcome::Welcome;

/// Name of the terminal "Clone in Terminal" runs in.
const CLONE_TERMINAL: &str = "Clone";

impl Workspace {
    /// The welcome screen view.
    pub fn welcome(&self) -> &Entity<Welcome> {
        &self.welcome
    }

    /// The body while the welcome screen shows: one unframed panel fills
    /// it, 6 px of canvas at the sides; the bottom panel (a terminal) can
    /// show under it. Takes the keyboard for the welcome screen when focus
    /// is nowhere in particular.
    pub(super) fn render_welcome_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut stray = vec![self.focus_handle.clone()];
        stray.extend(self.panes.iter().map(|p| p.focus_handle(cx)));
        self.welcome.read(cx).claim_focus(&stray, window, cx);
        let show_panel = self.bottom_panel_visible || self.sliding_closed(Slider::Panel);
        let bottom_focused = self
            .bottom_panel
            .focus_handle(cx)
            .contains_focused(window, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(GAP)
            .child(
                panel(false, cx)
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.welcome.clone())
                    .child(super::render::corner_caps(false, cx)),
            )
            .when(show_panel, |el| {
                let panel = self.render_bottom_panel(bottom_focused, cx);
                let slot = frame::bottom_slot(self.bottom_panel_height);
                el.child(self.slide_wrap(Slider::Panel, panel, slot, cx))
            })
    }

    /// Where a clone goes by default: like a new project (the
    /// `workspace.projects_folder` setting, else next to the current or
    /// most recent project), else `~/Projects` when it exists, else home.
    fn default_clone_folder(&self, cx: &gpui::App) -> PathBuf {
        let folder = self.default_projects_folder(cx);
        let home = std::env::home_dir();
        if home.as_deref() == Some(folder.as_path()) {
            for name in ["Projects", "projects"] {
                let candidate = folder.join(name);
                if candidate.is_dir() {
                    return candidate;
                }
            }
        }
        folder
    }

    /// Opens the Clone Repository dialog.
    pub(crate) fn show_clone_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        if self.clone_dialog.is_some() {
            return;
        }
        let parent = self.default_clone_folder(cx);
        let dialog = cx.new(|cx| CloneDialog::new(parent, cx));
        let sub = cx.subscribe_in(&dialog, window, |ws, _, event: &CloneEvent, window, cx| {
            ws.clone_dialog = None;
            match event.clone() {
                CloneEvent::Cloned(path) => {
                    let name = arcode_config::state::project_name(&path);
                    ws.show_toast_as(format!("Cloned {name}"), super::ToastLevel::Info, cx);
                    ws.open_project(path, window, cx);
                }
                CloneEvent::InTerminal(request) => ws.clone_in_terminal(&request, window, cx),
                CloneEvent::Dismissed => ws.refocus_after_overlay(window, cx),
            }
            cx.notify();
        });
        self.subs.set("clone", sub);
        window.focus(&dialog.focus_handle(cx));
        self.clone_dialog = Some(dialog);
        cx.notify();
    }

    /// The open Clone Repository dialog (tests).
    pub fn clone_dialog(&self) -> Option<&Entity<CloneDialog>> {
        self.clone_dialog.as_ref()
    }

    /// Focus after an overlay closed: the welcome screen while it shows,
    /// else the active pane.
    fn refocus_after_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shows_welcome(cx) {
            window.focus(&self.welcome.focus_handle(cx));
        } else {
            self.active_pane().read(cx).focus(window, cx);
        }
    }

    /// Runs `git clone` in a terminal of the bottom panel, where git can
    /// ask for a password or a host key confirmation.
    fn clone_in_terminal(
        &mut self,
        request: &CloneRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shell = crate::terminal_panel::configured_shell(cx);
        let in_parent = request.parent.is_dir();
        let command = terminal_command(shell, request, in_parent);
        let cwd = in_parent.then(|| request.parent.clone());
        self.bottom_panel_visible = true;
        self.bottom_panel.update(cx, |panel, cx| {
            panel.run_named(CLONE_TERMINAL, &command, cwd, None, window, cx)
        });
        self.show_toast_as(
            format!(
                "Cloning {} in the terminal. Open the folder when it is done.",
                request.folder
            ),
            super::ToastLevel::Info,
            cx,
        );
        cx.notify();
    }

    /// Clone Repository and Clear Recent Projects.
    pub(super) fn register_clone_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|ws, _: &CloneRepository, window, cx| ws.show_clone_dialog(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &ClearRecentProjects, _, cx| {
            let removed = app_state::clear_recent(cx);
            let message = match removed {
                0 => "No recent projects to clear (pinned ones stay).".to_owned(),
                1 => "Cleared 1 recent project.".to_owned(),
                n => format!("Cleared {n} recent projects."),
            };
            ws.welcome.update(cx, |w, cx| w.recheck(cx));
            ws.show_toast_as(message, super::ToastLevel::Info, cx);
        }))
    }

    /// Adds the dialog overlay to `root` while it is open. A click beside
    /// it closes it, unless a clone is running.
    pub(super) fn render_clone_overlay(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(dialog) = &self.clone_dialog else {
            return root;
        };
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(dialog.clone());
        root.child(
            crate::picker_frame::picker_layer("clone-backdrop")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| {
                        let running = ws
                            .clone_dialog
                            .as_ref()
                            .is_some_and(|d| d.read(cx).is_running());
                        if !running {
                            ws.clone_dialog = None;
                            ws.refocus_after_overlay(window, cx);
                            cx.notify();
                        }
                    }),
                )
                .child(fade_in("clone-anim", panel, cx)),
        )
    }
}
