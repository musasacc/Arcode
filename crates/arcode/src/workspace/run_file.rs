//! Run Current File and Debug Current File: the active editor's file run
//! or debugged on its own with its language's toolchain
//! (`arcode_project::file_command`), for folders without a project file
//! to detect run commands from, and the notice that says why nothing runs.
//!
//! The Run button, Ctrl+F5 and F5 fall back to these when the project has
//! no run command or launch configuration, so they always do something.

use std::path::Path;

use arcode_project::{
    FileLanguage, FileRunInput, RunCommand, RunKind, RunSource, SystemTools, debug_target,
    debug_unsupported_message, file_command, unsupported_message,
};
use gpui::{App, AppContext, Context, SharedString, Window};

use super::{ToastLevel, Workspace};
use crate::debugger::actions::{EditRunCommand, OpenLaunchConfig};
use crate::debugger::launch::{LaunchChoice, LaunchKind};
use crate::debugger::runnables::file_build_dir;

/// The terminal tab single files run in: one tab for all of them (a new
/// one only while the last run is still going), so running file after
/// file does not fill the tab row.
pub(crate) const FILE_RUN_TITLE: &str = "run file";

/// Whether `command` runs one source file on its own (Run Current File, a
/// gutter Run on a main function outside a project's targets, a Python
/// script).
fn is_single_file_run(command: &RunCommand) -> bool {
    match command.source {
        RunSource::File => true,
        RunSource::Python => command.kind == RunKind::Run,
        _ => false,
    }
}

/// The file's name for labels and messages.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl Workspace {
    /// The active editor's file name when Run Current File knows its
    /// language (what the Run button runs without a run command).
    pub fn runnable_file_name(&self, cx: &App) -> Option<String> {
        let editor = self.active_editor(cx)?;
        let e = editor.read(cx);
        let path = e.path()?;
        FileLanguage::detect(e.language().id, path).map(|_| name_of(path))
    }

    /// Runs the active editor's file in a terminal named `run <file>`.
    pub(crate) fn run_current_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_current_file(false, window, cx);
    }

    /// Debugs the active editor's file.
    pub(crate) fn debug_current_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_current_file(true, window, cx);
    }

    fn start_current_file(&mut self, debug: bool, window: &mut Window, cx: &mut Context<Self>) {
        let verb = if debug { "debug" } else { "run" };
        let Some(editor) = self.active_editor(cx) else {
            let message = if debug {
                "Nothing to debug: open a file to debug it, or add a configuration to \
                 .arcode/launch.toml."
            } else {
                "Nothing to run: open a file to run it, or set a run command."
            };
            self.nothing_to_run(debug, message, cx);
            return;
        };
        let (path, language_id, language_name, dirty) = {
            let e = editor.read(cx);
            let path = e.path().map(Path::to_path_buf);
            (path, e.language().id, e.language().name, e.is_dirty())
        };
        let Some(path) = path else {
            self.nothing_to_run(debug, format!("Save the file to {verb} it."), cx);
            return;
        };
        let language = FileLanguage::detect(language_id, &path);
        let Some(language) = language.filter(|l| !debug || l.can_debug()) else {
            let message = if debug {
                debug_unsupported_message(language, language_name, &path)
            } else {
                unsupported_message(&path, language_name)
            };
            self.nothing_to_run(debug, message, cx);
            return;
        };
        // What runs is the file on disk.
        if dirty && let Err(e) = editor.update(cx, |e, cx| e.save(cx)) {
            let name = name_of(&path);
            self.show_toast(format!("Cannot save {name}: {e}"), cx);
            return;
        }
        let root = self.project_root().map(Path::to_path_buf);
        let file = path.clone();
        let job = cx.background_spawn(async move {
            let build = file_build_dir();
            let input = FileRunInput {
                file: &file,
                language_id,
                project_root: root.as_deref(),
                build_dir: &build,
            };
            file_command(&input, &SystemTools)
        });
        self.debug.run.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update_in(cx, |ws, window, cx| match result {
                Err(message) => ws.nothing_to_run(debug, message, cx),
                Ok(command) if debug => ws.debug_file_command(&command, &path, language, cx),
                Ok(command) => ws.run_command_in_terminal(FILE_RUN_TITLE, &command, window, cx),
            })
            .ok();
        }));
    }

    /// Runs `command` in a terminal tab named `title` (single files share
    /// [`FILE_RUN_TITLE`]), after creating the folder a compiled single
    /// file goes to.
    pub(super) fn run_command_in_terminal(
        &mut self,
        title: &str,
        command: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = if is_single_file_run(command) {
            FILE_RUN_TITLE
        } else {
            title
        };
        // Tests check what would run without starting a shell.
        #[cfg(test)]
        {
            let _ = (window, cx);
            let line = command.command_line();
            self.debug.run.ran.push((title.to_string(), line));
        }
        #[cfg(not(test))]
        self.start_in_terminal(title, command, window, cx);
    }

    #[cfg(not(test))]
    fn start_in_terminal(
        &mut self,
        title: &str,
        command: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = arcode_project::create_output_dir(command) {
            self.show_toast(format!("Cannot create the build folder: {e}"), cx);
            return;
        }
        self.bottom_panel_visible = true;
        let shell = crate::terminal_panel::configured_shell(cx);
        let (line, cwd) = (command.command_line_in(shell), command.cwd.clone());
        self.bottom_panel.update(cx, |p, cx| {
            p.run_named(title, &line, Some(cwd), None, window, cx);
        });
        cx.notify();
    }

    fn debug_file_command(
        &mut self,
        command: &RunCommand,
        path: &Path,
        language: FileLanguage,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = debug_target(command) else {
            let message = debug_unsupported_message(Some(language), language.name(), path);
            self.nothing_to_run(true, message, cx);
            return;
        };
        let choice = LaunchChoice {
            name: format!("debug {}", name_of(path)),
            kind: LaunchKind::Target(Box::new(target)),
        };
        let file = path.to_path_buf();
        self.debug
            .debugger
            .update(cx, |d, cx| d.start(choice, Some(file), cx));
    }

    /// Says why nothing runs. With a project open the notice has a button:
    /// "Set run command" for Run, the launch file for Debug.
    pub(crate) fn nothing_to_run(
        &mut self,
        debug: bool,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let level = ToastLevel::Warning;
        let Some(root) = self.project_root() else {
            self.show_toast_as(message, level, cx);
            return;
        };
        if debug {
            let label = if root.join(arcode_dap::LAUNCH_FILE).exists() {
                "Open launch.toml"
            } else {
                "Create launch.toml"
            };
            self.show_toast_with_action(message, level, label, Box::new(OpenLaunchConfig), cx);
        } else {
            self.show_toast_with_action(
                message,
                level,
                "Set run command",
                Box::new(EditRunCommand),
                cx,
            );
        }
    }
}
