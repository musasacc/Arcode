//! Showing the New Project dialog and acting on it.

use std::path::PathBuf;

use gpui::{AppContext, Context, Div, Focusable, MouseButton, Stateful, Window, div, prelude::*};

use super::Workspace;
use super::overlays::fade_in;
use crate::app_state;
use crate::new_project::{NewProjectDialog, NewProjectEvent};
use crate::path_prompt::expand_home;

impl Workspace {
    /// Where new projects go by default: `workspace.projects_folder`, else
    /// next to the current (or most recent) project, else the home folder.
    fn default_projects_folder(&self, cx: &gpui::App) -> PathBuf {
        if let Some(folder) = &arcode_ui_kit::settings(cx).workspace.projects_folder {
            return expand_home(folder);
        }
        self.project_root()
            .map(|p| p.to_path_buf())
            .or_else(|| {
                app_state::recent_projects(cx)
                    .first()
                    .map(|p| p.path.clone())
            })
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Opens the New Project dialog.
    pub(crate) fn show_new_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.default_projects_folder(cx);
        let dialog = cx.new(|cx| NewProjectDialog::new(parent, cx));
        let sub = cx.subscribe_in(
            &dialog,
            window,
            |ws, _, event: &NewProjectEvent, window, cx| {
                ws.new_project = None;
                match event.clone() {
                    NewProjectEvent::Created {
                        path,
                        command,
                        new_window,
                    } => ws.open_new_project(path, command, new_window, window, cx),
                    NewProjectEvent::Dismissed => ws.active_pane().read(cx).focus(window, cx),
                }
                cx.notify();
            },
        );
        self.subs.set("new_project", sub);
        window.focus(&dialog.focus_handle(cx));
        self.new_project = Some(dialog);
        cx.notify();
    }

    /// The open New Project dialog (tests).
    pub fn new_project_dialog(&self) -> Option<&gpui::Entity<NewProjectDialog>> {
        self.new_project.as_ref()
    }

    /// Opens the created project here or in a new window, then runs the
    /// template's command in its terminal.
    fn open_new_project(
        &mut self,
        path: PathBuf,
        command: Option<String>,
        new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if new_window {
            cx.defer(move |cx| {
                if let Some(handle) = crate::windows::open_window(Some(path), None, cx)
                    && let Some(command) = command
                {
                    handle
                        .update(cx, |ws, window, cx| {
                            ws.run_in_terminal(&command, window, cx)
                        })
                        .ok();
                }
            });
            return;
        }
        self.confirm_discard(window, cx, move |ws, window, cx| {
            ws.open_project_now(&path, window, cx);
            if let Some(command) = &command {
                ws.run_in_terminal(command, window, cx);
            }
        });
    }

    /// Shows the terminal and runs `command` there.
    pub(crate) fn run_in_terminal(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bottom_panel_visible = true;
        self.bottom_panel
            .update(cx, |panel, cx| panel.run_command(command, window, cx));
        cx.notify();
    }

    /// Adds the dialog overlay to `root` while it is open.
    pub(super) fn render_new_project_overlay(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(dialog) = &self.new_project else {
            return root;
        };
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(dialog.clone());
        root.child(
            crate::picker_frame::picker_layer("new-project-backdrop")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| {
                        ws.new_project = None;
                        ws.active_pane().read(cx).focus(window, cx);
                        cx.notify();
                    }),
                )
                .child(fade_in("new-project-anim", panel, cx)),
        )
    }
}
