//! Debugger wiring of the workspace: the [`Debugger`], its sidebar and
//! panel, the breakpoint dialog, terminals for run commands and debuggees,
//! and the debug actions.

use std::path::{Path, PathBuf};

use arcode_buffer::Point;
use arcode_dap::RunInTerminalRequest;
use arcode_ui_kit::settings;
use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::app_state;
use crate::bottom_panel::{BottomPanel, BottomTab};
use crate::debugger::actions::*;
use crate::debugger::bp_dialog::{BpDialogEvent, BreakpointDialog};
use crate::debugger::control::Step;
use crate::debugger::panel::DebugPanel;
use crate::debugger::sidebar::{DebugSidebar, DebugSidebarEvent};
use crate::debugger::{Debugger, DebuggerEvent};

/// The debugger entities of a workspace.
pub struct DebugParts {
    /// Breakpoints, launch configurations and the session.
    pub debugger: Entity<Debugger>,
    /// The Run and Debug view.
    pub sidebar: Entity<DebugSidebar>,
    /// The breakpoint dialog, while open.
    pub bp_dialog: Option<Entity<BreakpointDialog>>,
    /// The title bar's run menu and the run command.
    pub run: super::run_menu::RunState,
}

impl DebugParts {
    /// Creates the entities, gives the bottom panel its Debug tab and
    /// returns the workspace's subscriptions.
    pub(super) fn new(
        bottom_panel: &Entity<BottomPanel>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> (Self, Vec<Subscription>) {
        let debugger = cx.new(Debugger::new);
        let sidebar = cx.new(|cx| DebugSidebar::new(debugger.clone(), cx));
        let panel = cx.new(|cx| DebugPanel::new(debugger.clone(), cx));
        bottom_panel.update(cx, |p, cx| p.set_debug(panel, cx));
        let observed = debugger.clone();
        let subs = vec![
            cx.subscribe_in(&debugger, window, on_debugger_event),
            cx.subscribe_in(
                &sidebar,
                window,
                |ws, _, event: &DebugSidebarEvent, window, cx| match event {
                    DebugSidebarEvent::Open(path, line) => {
                        let at = Point::new(*line, 0);
                        ws.open_at(path, at..at, window, cx);
                    }
                },
            ),
            cx.observe_global::<arcode_ui_kit::Settings>(move |_, cx| {
                observed.update(cx, |d, cx| d.settings_changed(cx));
            }),
        ];
        (
            Self {
                debugger,
                sidebar,
                bp_dialog: None,
                run: Default::default(),
            },
            subs,
        )
    }
}

fn on_debugger_event(
    ws: &mut Workspace,
    _: &Entity<Debugger>,
    event: &DebuggerEvent,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    match event {
        DebuggerEvent::Open { path, line, .. } => {
            let at = Point::new(*line, 0);
            ws.open_at(path, at..at, window, cx);
        }
        DebuggerEvent::RunCommand { title, command } => {
            ws.bottom_panel_visible = true;
            let line = command.command_line();
            let cwd = Some(command.cwd.clone());
            ws.bottom_panel.update(cx, |p, cx| {
                p.run_named(title, &line, cwd, None, window, cx);
            });
            cx.notify();
        }
        DebuggerEvent::RunInTerminal { title, request } => {
            ws.run_debuggee_in_terminal(title, request, window, cx)
        }
        DebuggerEvent::Message(text) => ws.show_toast(text.clone(), cx),
        DebuggerEvent::Started => {
            if settings(cx).debug.open_panel_on_start {
                ws.bottom_panel_visible = true;
                ws.bottom_panel
                    .update(cx, |p, cx| p.set_active_tab(BottomTab::Debug, cx));
                cx.notify();
            }
        }
        DebuggerEvent::BreakpointsChanged => ws.schedule_session_save(cx),
        DebuggerEvent::EditBreakpoint { path, line } => {
            ws.open_breakpoint_dialog(path.clone(), *line, window, cx)
        }
    }
}

impl Workspace {
    /// The debugger.
    pub fn debugger(&self) -> &Entity<Debugger> {
        &self.debug.debugger
    }

    /// Path of the active editor's file.
    fn active_file(&self, cx: &gpui::App) -> Option<PathBuf> {
        let editor = self.active_editor(cx)?;
        editor.read(cx).path().map(Path::to_path_buf)
    }

    fn run_debuggee_in_terminal(
        &mut self,
        title: &str,
        request: &RunInTerminalRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args = &request.arguments;
        let env: Vec<(String, Option<String>)> =
            args.env.clone().unwrap_or_default().into_iter().collect();
        let line = super::debuggee::debuggee_command_line(&args.args, &env);
        let cwd = (!args.cwd.is_empty()).then(|| PathBuf::from(&args.cwd));
        if line.is_empty() {
            if let Err(e) = request.fail("runInTerminal without a program") {
                log::warn!("runInTerminal: {e}");
            }
            return;
        }
        self.bottom_panel_visible = true;
        let request = request.clone();
        let answer: crate::terminal_panel::OnStarted = Box::new(move |terminal, cx| {
            let result = match terminal {
                Some(t) => request.respond(None, t.read(cx).terminal().child_pid()),
                None => request.fail("Arcode could not start a terminal"),
            };
            if let Err(e) = result {
                log::warn!("runInTerminal: {e}");
            }
        });
        self.bottom_panel.update(cx, |p, cx| {
            p.run_named(title, &line, cwd, Some(answer), window, cx);
            // The terminal stays on the Terminal tab; the Debug tab is
            // where the session is controlled.
            p.set_active_tab(BottomTab::Debug, cx);
        });
        cx.notify();
    }

    /// Opens the breakpoint dialog for `path:line`.
    pub(crate) fn open_breakpoint_dialog(
        &mut self,
        path: PathBuf,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = match self.project_root() {
            Some(root) => crate::project::relative_display(root, &path),
            None => path.display().to_string(),
        };
        let existing = self.debug.debugger.read(cx).book().at(&path, line).cloned();
        let dialog = cx.new(|cx| BreakpointDialog::new(path, line, display, existing.as_ref(), cx));
        let sub = cx.subscribe_in(
            &dialog,
            window,
            |ws, _, event: &BpDialogEvent, window, cx| {
                match event {
                    BpDialogEvent::Save {
                        path,
                        line,
                        condition,
                        hit_condition,
                        log_message,
                    } => {
                        let (c, h, l) = (
                            Some(condition.clone()),
                            Some(hit_condition.clone()),
                            Some(log_message.clone()),
                        );
                        ws.debug.debugger.update(cx, |d, cx| {
                            d.set_breakpoint_options(path, *line, c, h, l, cx)
                        });
                    }
                    BpDialogEvent::Remove { path, line } => {
                        let id = ws
                            .debug
                            .debugger
                            .read(cx)
                            .book()
                            .at(path, *line)
                            .map(|b| b.id);
                        if let Some(id) = id {
                            ws.debug
                                .debugger
                                .update(cx, |d, cx| d.remove_breakpoint(id, cx));
                        }
                    }
                    BpDialogEvent::Dismissed => {}
                }
                ws.close_breakpoint_dialog(window, cx);
            },
        );
        self.subs.set("bp_dialog", sub);
        window.focus(&dialog.focus_handle(cx));
        self.debug.bp_dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn close_breakpoint_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.debug.bp_dialog.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    /// The cursor line of the active editor with its file.
    fn cursor_location(&self, cx: &gpui::App) -> Option<(PathBuf, usize)> {
        let editor = self.active_editor(cx)?;
        let e = editor.read(cx);
        Some((e.path()?.to_path_buf(), e.cursor_position().line))
    }

    /// Points the debugger at the project (with its saved breakpoints).
    pub(super) fn set_debug_project(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        let session = root.as_deref().and_then(|r| app_state::session(r, cx));
        let saved = session
            .as_ref()
            .map(|s| s.breakpoints.clone())
            .unwrap_or_default();
        self.debug.run.custom = session.and_then(|s| s.run_command);
        self.debug
            .debugger
            .update(cx, |d, cx| d.set_project(root.clone(), &saved, cx));
        self.detect_run_command(root, cx);
    }

    fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        self.debug.debugger.update(cx, |d, cx| d.step(step, cx));
    }

    pub(super) fn register_debug_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(|ws, _: &StartOrContinue, _, cx| {
            let file = ws.active_file(cx);
            ws.debug
                .debugger
                .update(cx, |d, cx| d.start_or_continue(file, cx));
        }))
        .on_action(cx.listener(|ws, _: &Continue, _, cx| ws.step(Step::Continue, cx)))
        .on_action(cx.listener(|ws, _: &Pause, _, cx| ws.step(Step::Pause, cx)))
        .on_action(cx.listener(|ws, _: &StepOver, _, cx| ws.step(Step::Over, cx)))
        .on_action(cx.listener(|ws, _: &StepInto, _, cx| ws.step(Step::Into, cx)))
        .on_action(cx.listener(|ws, _: &StepOut, _, cx| ws.step(Step::Out, cx)))
        .on_action(cx.listener(|ws, _: &RestartDebugging, _, cx| {
            let file = ws.active_file(cx);
            ws.debug.debugger.update(cx, |d, cx| d.restart(file, cx));
        }))
        .on_action(cx.listener(|ws, _: &StopDebugging, _, cx| {
            ws.debug
                .debugger
                .update(cx, |d, cx| d.stop_session(false, cx));
        }))
        .on_action(cx.listener(|ws, _: &ToggleBreakpoint, _, cx| {
            match ws.cursor_location(cx) {
                Some((path, line)) => ws
                    .debug
                    .debugger
                    .update(cx, |d, cx| d.toggle_breakpoint(&path, line, cx)),
                None => ws.show_toast("Open a file to set a breakpoint", cx),
            }
        }))
        .on_action(cx.listener(
            |ws, _: &EditBreakpoint, window, cx| match ws.cursor_location(cx) {
                Some((path, line)) => ws.open_breakpoint_dialog(path, line, window, cx),
                None => ws.show_toast("Open a file to set a breakpoint", cx),
            },
        ))
        .on_action(cx.listener(|ws, _: &RemoveAllBreakpoints, _, cx| {
            ws.debug
                .debugger
                .update(cx, |d, cx| d.remove_all_breakpoints(cx));
        }))
        .on_action(cx.listener(|ws, _: &ShowDebugPanel, window, cx| {
            ws.bottom_panel_visible = true;
            ws.bottom_panel.update(cx, |p, cx| {
                p.set_active_tab(BottomTab::Debug, cx);
                p.focus(window, cx);
            });
            cx.notify();
        }))
        .on_action(cx.listener(|ws, _: &ShowRunAndDebug, window, cx| {
            ws.activity = Activity::Debug;
            ws.sidebar_visible = true;
            window.focus(&ws.debug.sidebar.focus_handle(cx));
            cx.notify();
        }))
        .on_action(
            cx.listener(|ws, _: &OpenLaunchConfig, window, cx| ws.open_launch_config(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &PickRunCommand, window, cx| ws.toggle_run_menu(window, cx)))
    }

    /// Opens `.arcode/launch.toml`, creating it from a template first.
    fn open_launch_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.show_toast("Open a project first", cx);
            return;
        };
        let path = root.join(arcode_dap::LAUNCH_FILE);
        if !path.exists() {
            let program = self.debug.run.default.as_ref().and_then(|c| {
                (c.source == arcode_project::RunSource::Cargo)
                    .then(|| format!("target/debug/{}", arcode_config::state::project_name(&root)))
            });
            let text = crate::debugger::launch::template(program.as_deref());
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, text));
            if let Err(e) = written {
                self.show_toast(format!("Cannot create {}: {e}", path.display()), cx);
                return;
            }
            self.debug.debugger.update(cx, |d, cx| d.reload_choices(cx));
        }
        self.open_file(&path, window, cx);
    }
}
