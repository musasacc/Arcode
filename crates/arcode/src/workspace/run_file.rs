//! Run Current File and Debug Current File: the active editor's file run
//! or debugged on its own with its language's toolchain
//! (`arcode_project::file_command`), for folders without a project file
//! to detect run commands from, and the notice that says why nothing runs.
//!
//! The Run button, Ctrl+F5 and F5 fall back to these when the project has
//! no run command or launch configuration, so they always do something.
//!
//! A compiled program goes next to its source by default (`run.output`),
//! where the file tree shows it; the command is worked out again right
//! before each run, so a name taken since is never written over.

use std::path::{Path, PathBuf};

use arcode_project::{
    FileLanguage, RunCommand, RunKind, RunSource, debug_target, debug_unsupported_message,
    unsupported_message,
};
use arcode_ui_kit::settings;
use gpui::{App, AppContext, Context, SharedString, Window};

use super::{ToastLevel, Workspace};
use crate::debugger::actions::{EditRunCommand, OpenLaunchConfig};
use crate::debugger::launch::{LaunchChoice, LaunchKind};
use crate::debugger::runnables::{compiled_source, single_file_run};

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

/// Notes that a build into `program` (next to its source) starts in a
/// terminal, then watches for the program it writes (once a second, for at
/// most [`arcode_project::BUILD_WINDOW`]) and records its exact time, so a
/// program put there later by something else is never written over. When
/// the last run in `terminals` is seen to end in the terminal the build
/// started in (`run`, set once it started) without the program, the build
/// failed and the note is dropped at once.
#[cfg(not(test))]
fn note_terminal_build(
    program: PathBuf,
    terminals: gpui::Entity<crate::terminal_panel::TerminalPanel>,
    run: std::rc::Rc<std::cell::Cell<Option<gpui::EntityId>>>,
    cx: &mut Context<Workspace>,
) {
    use arcode_project::{abandon_started_build, finish_started_build, record_build_started};
    let record = crate::debugger::runnables::built_record();
    cx.spawn(async move |_, cx| {
        let (r, p) = (record.clone(), program.clone());
        let started = match cx
            .background_spawn(async move { record_build_started(&r, &p) })
            .await
        {
            Ok(at) => at,
            Err(e) => return log::warn!("cannot note {} as built: {e}", program.display()),
        };
        let tick = std::time::Duration::from_secs(1);
        for _ in 0..arcode_project::BUILD_WINDOW.as_secs() {
            cx.background_executor().timer(tick).await;
            // Looked at before the program: a build that had ended by now
            // had written it by now. A later run in that terminal starts
            // only once this one is over.
            let ended = run.get().is_some_and(|id| {
                terminals
                    .read_with(cx, |t, _| {
                        t.last_run()
                            .is_some_and(|r| r.terminal() == id && r.has_ended())
                    })
                    .unwrap_or(false)
            });
            let (r, p) = (record.clone(), program.clone());
            let done = cx
                .background_spawn(async move {
                    match finish_started_build(&r, &p)? {
                        false if ended => abandon_started_build(&r, &p, started).map(|()| true),
                        recorded => Ok(recorded),
                    }
                })
                .await;
            match done {
                Ok(false) => {}
                Ok(true) => return,
                Err(e) => return log::warn!("cannot note {} as built: {e}", program.display()),
            }
        }
    })
    .detach();
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
        self.start_file(path, language_id, language, debug, window, cx);
    }

    /// Works out the command for `path` off the UI thread, says where its
    /// program went when that is not where the setting asks, then runs or
    /// debugs it.
    fn start_file(
        &mut self,
        path: PathBuf,
        language_id: &'static str,
        language: FileLanguage,
        debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.project_root().map(Path::to_path_buf);
        let output = settings(cx).run.output;
        let file = path.clone();
        let job = cx.background_spawn(async move {
            single_file_run(&file, language_id, root.as_deref(), output)
        });
        self.debug.run.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update_in(cx, |ws, window, cx| match result {
                Err(message) => ws.nothing_to_run(debug, message, cx),
                Ok(run) => {
                    if let Some(note) = run.note {
                        ws.show_toast_as(note, ToastLevel::Info, cx);
                    }
                    if debug {
                        ws.debug_file_command(&run.command, &path, language, cx)
                    } else {
                        ws.start_program(FILE_RUN_TITLE, &run.command, window, cx)
                    }
                }
            })
            .ok();
        }));
    }

    /// Runs `command` in a terminal tab named `title` (single files share
    /// [`FILE_RUN_TITLE`]). A program compiled from one file (a gutter Run
    /// on a loose `main`) is placed again first, as Run Current File does.
    pub(super) fn run_command_in_terminal(
        &mut self,
        title: &str,
        command: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if command.source == RunSource::File
            && let Some(file) = compiled_source(command)
            && let Some(language) = FileLanguage::from_path(&file)
        {
            self.start_file(file, "", language, false, window, cx);
            return;
        }
        self.start_program(title, command, window, cx);
    }

    /// Runs `command` in a terminal tab, after creating the folder a
    /// compiled single file goes to and noting a program built next to its
    /// source as Arcode's.
    fn start_program(
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
        // The terminal the command starts in (it may start a moment later).
        let run = std::rc::Rc::new(std::cell::Cell::new(None));
        if let Some(program) = crate::debugger::runnables::program_next_to_source(command) {
            let terminals = self.bottom_panel.read(cx).terminals().clone();
            note_terminal_build(program, terminals, run.clone(), cx);
        }
        let on_started: crate::terminal_panel::OnStarted =
            Box::new(move |terminal, _| run.set(terminal.map(|t| t.entity_id())));
        self.bottom_panel_visible = true;
        let shell = crate::terminal_panel::configured_shell(cx);
        let (line, cwd) = (command.command_line_in(shell), command.cwd.clone());
        self.bottom_panel.update(cx, |p, cx| {
            p.run_named(title, &line, Some(cwd), Some(on_started), window, cx);
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
