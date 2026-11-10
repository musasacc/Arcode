//! Terminals that run one program directly (no shell), for the package
//! manager commands of the Packages view: the arguments reach the program
//! exactly as given, nothing is interpreted by a shell. The tab stays
//! open after the program exits so its output can be read.

use arcode_project::deps::PackageCommand;
use arcode_terminal::{Terminal, TerminalOptions};
use arcode_terminal_view::{TerminalView, TerminalViewEvent};
use arcode_ui_kit::settings;
use gpui::{App, AppContext, Context, EntityId, SharedString, Window};

use super::TerminalPanel;

/// Called once with the exit code (`None` when killed) when the program
/// of a [`TerminalPanel::run_program`] tab exits, or when its tab is closed
/// first.
pub type OnExit = Box<dyn FnOnce(Option<i32>, &mut App)>;

impl TerminalPanel {
    /// Starts `command` (its program with its arguments, in its folder) in
    /// a new terminal tab named after its command line and shows it. When
    /// the program exits the tab is kept (its name gets "(done)") and
    /// `on_exit` runs. Fails when the program cannot be started (e.g. it is
    /// not installed).
    pub fn run_program(
        &mut self,
        command: &PackageCommand,
        on_exit: Option<OnExit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let options = TerminalOptions {
            shell: Some(command.program.clone()),
            args: command.args.clone(),
            // Programs on Windows reject the `\\?\` form canonical paths have.
            cwd: Some(arcode_project::files::external_path(&command.cwd)),
            scrollback: settings(cx).terminal.scrollback,
            ..TerminalOptions::default()
        };
        let (terminal, events) = Terminal::spawn(options).map_err(|e| format!("{e:#}"))?;
        let view = cx.new(|cx| TerminalView::with_terminal(terminal, events, window, cx));
        let sub = cx.subscribe_in(
            &view,
            window,
            move |panel, view, event, _, cx| match event {
                TerminalViewEvent::Exited => {
                    let id = view.entity_id();
                    let code = view.read(cx).exit_code();
                    panel.run_terminal_exited(id, code);
                    let status = exit_label(code);
                    if let Some(name) = panel.names.get(&id).cloned() {
                        panel
                            .names
                            .insert(id, SharedString::from(format!("{name} ({status})")));
                    }
                    if let Some(f) = panel.on_exit.remove(&id) {
                        f(code, cx);
                    }
                    cx.notify();
                }
                TerminalViewEvent::TitleChanged => cx.notify(),
                TerminalViewEvent::Bell => {}
            },
        );
        self.subscriptions.push((view.entity_id(), sub));
        if let Some(f) = on_exit {
            self.on_exit.insert(view.entity_id(), f);
        }
        self.names.insert(
            view.entity_id(),
            SharedString::from(command.display.clone()),
        );
        self.track_run(&command.display, &view, true, false, cx);
        self.terminals.push(view);
        self.active = self.terminals.len() - 1;
        self.error = None;
        cx.notify();
        Ok(())
    }

    /// The tab `id` was closed: when its program had not reported its exit
    /// yet, its `on_exit` runs now with `code` (`None`: it was stopped), so
    /// a caller waiting for the exit is never left waiting.
    pub(super) fn program_closed(&mut self, id: EntityId, code: Option<i32>, cx: &mut App) {
        if let Some(f) = self.on_exit.remove(&id) {
            f(code, cx);
        }
    }

    /// Every tab was closed (the project changed): their programs stopped.
    pub(super) fn programs_closed(&mut self, cx: &mut App) {
        for (_, f) in self.on_exit.drain() {
            f(None, cx);
        }
    }
}

/// How a finished program's tab is labelled: "done" after success, else
/// the exit code ("exit 1"), or "stopped" when it was killed.
pub fn exit_label(code: Option<i32>) -> String {
    match code {
        Some(0) => "done".to_string(),
        Some(code) => format!("exit {code}"),
        None => "stopped".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::exit_label;

    #[test]
    fn exit_labels() {
        assert_eq!(exit_label(Some(0)), "done");
        assert_eq!(exit_label(Some(101)), "exit 101");
        assert_eq!(exit_label(None), "stopped");
    }
}
