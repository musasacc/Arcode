//! The debugger and run buttons of a workspace.
//!
//! [`Debugger`] (one entity per workspace) owns the project's breakpoints,
//! its launch configurations, the gutter marks of every open editor
//! (breakpoints, run buttons, the stopped line) and at most one running
//! [`arcode_dap`] session. Views (the Debug panel, the Run and Debug
//! sidebar, the run menu and the breakpoint dialog) read it; the workspace
//! reacts to its [`DebuggerEvent`]s (open a location, run a command in a
//! terminal, show a message).

pub mod actions;
pub mod bp_dialog;
mod bp_ops;
pub mod breakpoints;
pub mod console;
pub mod control;
mod editors;
mod events;
mod inline_values;
mod inspect;
pub mod launch;
mod line_follow;
pub mod panel;
pub mod prepare;
#[cfg(test)]
mod review_debugger;
pub mod run_menu;
pub mod runnables;
pub mod session;
pub mod sidebar;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use arcode_dap::RunInTerminalRequest;
use arcode_project::RunCommand;
use gpui::{AppContext, Context, EventEmitter, Task};

use breakpoints::BreakpointBook;
use console::Console;
use editors::TrackedEditor;
use launch::LaunchChoice;
use prepare::Cancel;
use session::{Session, WatchValue};

/// What the workspace should do for the debugger.
#[derive(Clone)]
pub enum DebuggerEvent {
    /// Open `path` at a zero-based line (a stop, a clicked frame).
    Open {
        /// The file.
        path: PathBuf,
        /// Zero-based line.
        line: usize,
        /// Move keyboard focus to the editor.
        focus: bool,
    },
    /// Run a command in a terminal tab named `title`.
    RunCommand {
        /// Tab name.
        title: String,
        /// The command.
        command: RunCommand,
    },
    /// The adapter asks to start the debuggee in a terminal.
    RunInTerminal {
        /// Tab name.
        title: String,
        /// The request to answer.
        request: RunInTerminalRequest,
    },
    /// Show a short message.
    Message(String),
    /// A session is starting: show the Debug panel.
    Started,
    /// Breakpoints changed (save the session).
    BreakpointsChanged,
    /// Open the breakpoint dialog for `path:line`.
    EditBreakpoint {
        /// The file.
        path: PathBuf,
        /// Zero-based line.
        line: usize,
    },
}

/// A launch being prepared (adapter lookup, build).
struct Preparing {
    name: String,
    cancel: Cancel,
    _task: Task<()>,
}

impl Drop for Preparing {
    /// A build nobody waits for any more (the window closed, another
    /// session started) is stopped with everything it started.
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The workspace's debugger.
pub struct Debugger {
    root: Option<PathBuf>,
    book: BreakpointBook,
    choices: Vec<LaunchChoice>,
    warnings: Vec<String>,
    selected: usize,
    choices_task: Option<Task<()>>,
    editors: Vec<TrackedEditor>,
    session: Option<Session>,
    preparing: Option<Preparing>,
    console: Console,
    watches: Vec<(String, WatchValue)>,
    /// The location of the selected frame while stopped.
    stopped_at: Option<(PathBuf, usize)>,
    /// Bumped when the console changes (views scroll to the end).
    console_version: u64,
    /// Open nodes of the variables and watch trees.
    tree: inspect::TreeState,
}

impl EventEmitter<DebuggerEvent> for Debugger {}

impl Debugger {
    /// A debugger without a project.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            root: None,
            book: BreakpointBook::default(),
            choices: Vec::new(),
            warnings: Vec::new(),
            selected: 0,
            choices_task: None,
            editors: Vec::new(),
            session: None,
            preparing: None,
            console: Console::default(),
            watches: Vec::new(),
            stopped_at: None,
            console_version: 0,
            tree: Default::default(),
        }
    }

    /// Switches to the project at `root` with its saved breakpoints; ends
    /// a running session.
    pub fn set_project(
        &mut self,
        root: Option<PathBuf>,
        saved: &[arcode_config::SavedBreakpoint],
        cx: &mut Context<Self>,
    ) {
        self.stop_session(true, cx);
        self.root = root;
        self.book = BreakpointBook::from_saved(saved);
        self.console.clear();
        self.watches.clear();
        self.selected = 0;
        self.reload_choices(cx);
        self.refresh_all_editors(cx);
        cx.notify();
    }

    /// The project folder.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The breakpoints.
    pub fn book(&self) -> &BreakpointBook {
        &self.book
    }

    /// The launch configurations.
    pub fn choices(&self) -> &[LaunchChoice] {
        &self.choices
    }

    /// Problems found in `launch.toml`.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Index of the selected configuration.
    pub fn selected(&self) -> usize {
        self.selected.min(self.choices.len().saturating_sub(1))
    }

    /// Selects a configuration.
    pub fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.choices.len() && ix != self.selected {
            self.selected = ix;
            cx.notify();
        }
    }

    /// The running session, if any.
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Name of the configuration being prepared (built), if any.
    pub fn preparing(&self) -> Option<&str> {
        self.preparing.as_ref().map(|p| p.name.as_str())
    }

    /// Whether a session runs or is being prepared.
    pub fn is_active(&self) -> bool {
        self.preparing.is_some() || self.session.as_ref().is_some_and(|s| !s.is_terminated())
    }

    /// The console.
    pub fn console(&self) -> &Console {
        &self.console
    }

    /// Changes whenever the console gets lines.
    pub fn console_version(&self) -> u64 {
        self.console_version
    }

    /// Watch expressions and their values.
    pub fn watches(&self) -> &[(String, WatchValue)] {
        &self.watches
    }

    /// Where the selected frame is stopped.
    pub fn stopped_at(&self) -> Option<&(PathBuf, usize)> {
        self.stopped_at.as_ref()
    }

    /// Reads the launch configurations again (after `launch.toml` or a
    /// package file was saved). Runs in the background.
    pub fn reload_choices(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            self.choices.clear();
            self.warnings.clear();
            self.choices_task = None;
            return;
        };
        let load = cx.background_spawn(async move { launch::load_choices(&root) });
        self.choices_task = Some(cx.spawn(async move |this, cx| {
            let (choices, warnings) = load.await;
            this.update(cx, |d, cx| {
                let selected = d.choices.get(d.selected).map(|c| c.name.clone());
                if d.choices != choices || d.warnings != warnings {
                    d.selected = selected
                        .and_then(|name| choices.iter().position(|c| c.name == name))
                        .unwrap_or(0);
                    d.choices = choices;
                    d.warnings = warnings;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn console_changed(&mut self, cx: &mut Context<Self>) {
        self.console_version += 1;
        cx.notify();
    }

    /// Adds an informational console line.
    pub(crate) fn info(&mut self, kind: console::LineKind, text: String, cx: &mut Context<Self>) {
        self.console.line(kind, text);
        self.console_changed(cx);
    }

    /// Empties the console.
    pub fn clear_console(&mut self, cx: &mut Context<Self>) {
        self.console.clear();
        self.console_changed(cx);
    }
}
