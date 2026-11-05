//! The title bar's Run button: the project's run command (detected by
//! `arcode-project`, or one the user picked or typed), else the current
//! file (see `run_file.rs`), and its dropdown listing every detected
//! command and Run / Debug Current File.

use std::path::{Path, PathBuf};

use arcode_project::{RunCommand, default_run_command, detect_run_commands};
use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    AppContext, Context, Div, Entity, Focusable, MouseButton, SharedString, Stateful, Task, Window,
    div, prelude::*, px,
};

use super::Workspace;
use crate::debugger::actions::{
    DebugCurrentFile, OpenLaunchConfig, RunCurrentFile, ShowRunAndDebug,
};
use crate::text_prompt::{TextPrompt, TextPromptEvent};
use crate::title_bar::TITLE_BAR_HEIGHT;

/// Run command state of a workspace.
#[derive(Default)]
pub struct RunState {
    /// The detected default command.
    pub default: Option<RunCommand>,
    /// A command line the user picked or typed (saved with the session).
    pub custom: Option<String>,
    /// The dropdown's commands while it is open (`None`: closed).
    pub menu: Option<Vec<RunCommand>>,
    /// The dropdown's commands were listed (an empty list means none).
    pub menu_loaded: bool,
    /// The "Edit Run Command" dialog.
    pub prompt: Option<Entity<TextPrompt>>,
    /// Detects the default command.
    task: Option<Task<()>>,
    /// Lists the dropdown's commands.
    menu_task: Option<Task<()>>,
    /// Finds the toolchain for Run / Debug Current File.
    pub(super) file_task: Option<Task<()>>,
    /// `(title, command line)` of each terminal run (tests only).
    #[cfg(test)]
    pub ran: Vec<(String, String)>,
}

impl Workspace {
    /// Detects the default run command in the background.
    pub(super) fn detect_run_command(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.debug.run.default = None;
        self.debug.run.menu = None;
        self.debug.run.menu_task = None;
        let Some(root) = root else {
            self.debug.run.task = None;
            return;
        };
        let detect = cx.background_spawn(async move { default_run_command(&root) });
        self.debug.run.task = Some(cx.spawn(async move |this, cx| {
            let command = detect.await;
            this.update(cx, |ws, cx| {
                ws.debug.run.default = command;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The command line the Run button runs, if any.
    pub fn run_command_line(&self) -> Option<String> {
        self.debug.run.custom.clone().or_else(|| {
            self.debug
                .run
                .default
                .as_ref()
                .map(RunCommand::command_line)
        })
    }

    /// Runs the project's run command in a terminal named after it; the
    /// current file when the project has none (or no project is open).
    pub(super) fn run_project_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.run_current_file(window, cx);
            return;
        };
        let (title, line, cwd) = match (&self.debug.run.custom, &self.debug.run.default) {
            (Some(line), _) => (line.clone(), line.clone(), root),
            (None, Some(cmd)) => (cmd.label.clone(), cmd.command_line(), cmd.cwd.clone()),
            (None, None) => {
                self.run_current_file(window, cx);
                return;
            }
        };
        self.bottom_panel_visible = true;
        self.bottom_panel.update(cx, |p, cx| {
            p.run_named(&title, &line, Some(cwd), None, window, cx);
        });
        cx.notify();
    }

    /// Opens or closes the Run dropdown (its commands load in the
    /// background).
    pub(super) fn toggle_run_menu(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.debug.run.menu.take().is_some() {
            self.debug.run.menu_task = None;
            cx.notify();
            return;
        }
        self.project_menu_open = false;
        self.debug.run.menu = Some(Vec::new());
        // Without a project there are no commands, only the current file.
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.debug.run.menu_loaded = true;
            cx.notify();
            return;
        };
        self.debug.run.menu_loaded = false;
        let detect = cx.background_spawn(async move { detect_run_commands(&root) });
        self.debug.run.menu_task = Some(cx.spawn(async move |this, cx| {
            let commands = detect.await;
            this.update(cx, |ws, cx| {
                if ws.debug.run.menu.is_some() {
                    ws.debug.run.menu = Some(commands);
                    ws.debug.run.menu_loaded = true;
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn pick_run_command(
        &mut self,
        command: RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.debug.run.menu = None;
        let line = command.command_line();
        let is_default = self
            .debug
            .run
            .default
            .as_ref()
            .is_some_and(|d| d.command_line() == line);
        self.debug.run.custom = (!is_default).then_some(line);
        if is_default || command.cwd.as_path() == self.project_root().unwrap_or(Path::new("")) {
            self.run_project_command(window, cx);
        } else {
            // Keep the folder the command belongs to for this run.
            self.bottom_panel_visible = true;
            let (label, line, cwd) = (command.label.clone(), command.command_line(), command.cwd);
            self.bottom_panel.update(cx, |p, cx| {
                p.run_named(&label, &line, Some(cwd), None, window, cx);
            });
        }
        self.schedule_session_save(cx);
        cx.notify();
    }

    /// Opens the dialog that sets the project's run command.
    pub(super) fn edit_run_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_root().is_none() {
            self.show_toast("Open a folder to set its run command", cx);
            return;
        }
        self.debug.run.menu = None;
        let initial = self.run_command_line().unwrap_or_default();
        let prompt = cx.new(|cx| {
            TextPrompt::new(
                "Run command",
                "Runs in a terminal in the project folder. Clear it and press Enter on an empty field to cancel.",
                IconName::Play,
                &initial,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws, _, event: &TextPromptEvent, window, cx| {
                ws.debug.run.prompt = None;
                if let TextPromptEvent::Confirmed(line) = event {
                    let is_default = ws
                        .debug
                        .run
                        .default
                        .as_ref()
                        .is_some_and(|d| d.command_line() == *line);
                    ws.debug.run.custom = (!is_default).then(|| line.clone());
                    ws.schedule_session_save(cx);
                    ws.run_project_command(window, cx);
                } else {
                    ws.active_pane().read(cx).focus(window, cx);
                }
                cx.notify();
            },
        );
        self.subs.set("run_prompt", sub);
        window.focus(&prompt.focus_handle(cx));
        self.debug.run.prompt = Some(prompt);
        cx.notify();
    }

    fn run_menu_row(
        id: impl Into<gpui::ElementId>,
        glyph: IconName,
        label: SharedString,
        detail: Option<SharedString>,
        cx: &gpui::App,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        // A popover row (docs/DESIGN.md 6.12): 26 tall, radius 6, solid hover.
        arcode_ui_kit::list_row(id, false, cx)
            .mx(px(0.))
            .h(px(26.))
            .gap(px(8.))
            .child(icon(glyph).size(px(14.)).color(c.text_muted))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when_some(detail, |el, d| {
                el.child(
                    div()
                        .flex_none()
                        .text_size(arcode_ui_kit::type_scale(cx).meta)
                        .text_color(c.text_muted)
                        .child(d),
                )
            })
    }

    /// The open dropdown and the run command dialog, over `root`.
    pub(super) fn render_run_overlays(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let mut root = root;
        if let Some(commands) = &self.debug.run.menu {
            let current = self.run_command_line();
            let mut menu = arcode_ui_kit::popover(cx)
                .w(px(380.))
                .text_size(arcode_ui_kit::type_scale(cx).body)
                .text_color(c.text)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            if commands.is_empty() {
                let note = if !self.debug.run.menu_loaded {
                    "Looking for run commands…"
                } else if self.project_root().is_some() {
                    "No run commands found in this project"
                } else {
                    "Open a folder to see its run commands"
                };
                menu = menu.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(note),
                );
            }
            for (ix, command) in commands.iter().enumerate() {
                let line = command.command_line();
                let glyph = if current.as_deref() == Some(line.as_str()) {
                    IconName::Check
                } else {
                    IconName::Play
                };
                let kind = match command.kind {
                    arcode_project::RunKind::Run => "run",
                    arcode_project::RunKind::Test => "test",
                    arcode_project::RunKind::Build => "build",
                };
                let picked = command.clone();
                menu = menu.child(
                    Self::run_menu_row(("run-cmd", ix), glyph, line.into(), Some(kind.into()), cx)
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            ws.pick_run_command(picked.clone(), window, cx)
                        })),
                );
            }
            menu = menu.child(div().my(px(4.)).h(px(1.)).bg(c.border));
            let detail: SharedString = self
                .active_editor(cx)
                .and_then(|e| {
                    e.read(cx)
                        .path()?
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .map_or_else(|| "no file open".into(), Into::into);
            menu = menu
                .child(
                    Self::run_menu_row(
                        "run-file",
                        IconName::Play,
                        "Run current file".into(),
                        Some(detail.clone()),
                        cx,
                    )
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.debug.run.menu = None;
                        window.dispatch_action(Box::new(RunCurrentFile), cx);
                    })),
                )
                .child(
                    Self::run_menu_row(
                        "debug-file",
                        IconName::Bug,
                        "Debug current file".into(),
                        Some(detail),
                        cx,
                    )
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.debug.run.menu = None;
                        window.dispatch_action(Box::new(DebugCurrentFile), cx);
                    })),
                )
                .child(div().my(px(4.)).h(px(1.)).bg(c.border))
                .child(
                    Self::run_menu_row(
                        "run-edit",
                        IconName::Type,
                        "Edit run command…".into(),
                        None,
                        cx,
                    )
                    .on_click(cx.listener(|ws, _, window, cx| ws.edit_run_command(window, cx))),
                )
                .child(
                    Self::run_menu_row(
                        "run-debug",
                        IconName::Bug,
                        "Run and debug…".into(),
                        None,
                        cx,
                    )
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.debug.run.menu = None;
                        window.dispatch_action(Box::new(ShowRunAndDebug), cx);
                    })),
                )
                .child(
                    Self::run_menu_row(
                        "run-launch",
                        IconName::Settings,
                        "Open launch.toml".into(),
                        None,
                        cx,
                    )
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.debug.run.menu = None;
                        window.dispatch_action(Box::new(OpenLaunchConfig), cx);
                    })),
                );
            root = root.child(
                div()
                    .id("run-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, _, cx| {
                            ws.debug.run.menu = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            // Under the Run split button (its right edge is
                            // 82 px from the window's: padding, Settings,
                            // Debug and the gaps between them).
                            .top(px(TITLE_BAR_HEIGHT - 2.))
                            .right(px(82.))
                            .child(super::overlays::fade_in("run-menu-anim", menu, cx)),
                    ),
            );
        }
        let dialog: Option<gpui::AnyView> = self
            .debug
            .run
            .prompt
            .clone()
            .map(Into::into)
            .or_else(|| self.debug.bp_dialog.clone().map(Into::into));
        if let Some(dialog) = dialog {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(dialog);
            root = root.child(
                crate::picker_frame::picker_layer("debug-dialog-backdrop")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, window, cx| {
                            ws.debug.run.prompt = None;
                            ws.close_breakpoint_dialog(window, cx);
                            cx.notify();
                        }),
                    )
                    .child(super::overlays::fade_in("debug-dialog-anim", panel, cx)),
            );
        }
        root
    }
}
