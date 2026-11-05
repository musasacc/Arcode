//! Terminals with a fixed name, started for a run command or a debuggee.

use std::path::{Path, PathBuf};

use arcode_project::{Shell, quote_arg_in};
use arcode_terminal_view::TerminalView;
use gpui::{App, AppContext, Context, Entity, SharedString, Window};

use super::TerminalPanel;

/// Called with the terminal a named command runs in (`None` when no
/// terminal could be started).
pub type OnStarted = Box<dyn FnOnce(Option<&Entity<TerminalView>>, &mut App)>;

impl TerminalPanel {
    /// Runs `command` in the terminal tab named `title` (started in `cwd`
    /// if there is none, or if every one is busy running something) and
    /// shows it. An idle tab whose shell has moved to another folder
    /// (`cd`) goes back to `cwd` first. Whether a tab is busy is checked in
    /// the background (it can spawn `ps`), so the command may start a
    /// moment later; `on_started` gets the terminal then.
    pub fn run_in_named(
        &mut self,
        title: &str,
        command: &str,
        cwd: Option<PathBuf>,
        on_started: Option<OnStarted>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let candidates: Vec<(Entity<TerminalView>, u32)> = self
            .terminals
            .iter()
            .filter(|t| self.names.get(&t.entity_id()).is_some_and(|n| n == title))
            .filter_map(|t| {
                let view = t.read(cx);
                let pid = view.terminal().child_pid()?;
                (view.is_alive() && can_check_busy()).then(|| (t.clone(), pid))
            })
            .collect();
        let (title, command) = (title.to_string(), command.to_string());
        if candidates.is_empty() {
            self.start_named(title, &command, cwd, on_started, window, cx);
            return;
        }
        let pids: Vec<u32> = candidates.iter().map(|(_, pid)| *pid).collect();
        let want = cwd.clone();
        let check = cx.background_spawn(async move {
            let want = want.map(|dir| std::fs::canonicalize(&dir).unwrap_or(dir));
            pids.into_iter()
                .map(|pid| {
                    let busy = arcode_project::procs::is_running_command(pid);
                    let moved = want.as_deref().is_some_and(|want| {
                        arcode_project::procs::current_dir(pid).as_deref() != Some(want)
                    });
                    (busy, moved)
                })
                .collect::<Vec<(bool, bool)>>()
        });
        let shell = configured_shell(cx);
        cx.spawn_in(window, async move |this, cx| {
            let states = check.await;
            this.update_in(cx, |panel, window, cx| {
                let idle = candidates
                    .into_iter()
                    .zip(states)
                    .filter(|(_, (busy, _))| !busy)
                    .map(|((t, _), (_, moved))| (t, moved))
                    .find(|(t, _)| t.read(cx).is_alive() && panel.terminals.contains(t));
                match idle {
                    Some((terminal, moved)) => {
                        if let Some(ix) = panel.terminals.iter().position(|t| *t == terminal) {
                            panel.active = ix;
                        }
                        if moved && let Some(dir) = &cwd {
                            terminal.read(cx).run_command(&cd_line(shell, dir));
                        }
                        terminal.read(cx).run_command(&command);
                        panel.track_run(&title, &terminal, false, false, cx);
                        cx.notify();
                        if let Some(f) = on_started {
                            f(Some(&terminal), cx);
                        }
                    }
                    None => panel.start_named(title, &command, cwd, on_started, window, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn start_named(
        &mut self,
        title: String,
        command: &str,
        cwd: Option<PathBuf>,
        on_started: Option<OnStarted>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.new_terminal_in(cwd, window, cx);
        if let Some(terminal) = &terminal {
            terminal.read(cx).run_command(command);
            self.track_run(&title, terminal, false, true, cx);
            self.names
                .insert(terminal.entity_id(), SharedString::from(title));
        }
        if let Some(f) = on_started {
            f(terminal.as_ref(), cx);
        }
    }
}

/// The kind of shell the terminal starts (`[terminal] shell`, else the
/// platform default), for lines typed into it.
pub fn configured_shell(cx: &App) -> Shell {
    match &arcode_ui_kit::settings(cx).terminal.shell {
        Some(shell) => Shell::of_program(shell),
        None => Shell::of_program(&arcode_terminal::default_shell()),
    }
}

/// The line that changes `shell`'s directory to `dir`.
pub(crate) fn cd_line(shell: Shell, dir: &Path) -> String {
    let dir = arcode_project::files::external_path(dir);
    let dir = dir.to_string_lossy();
    let quoted = quote_arg_in(shell, &dir);
    match shell {
        Shell::Cmd => format!("cd /d {quoted}"),
        _ => format!("cd {quoted}"),
    }
}

/// Whether an idle named terminal can be told from a busy one (Unix:
/// the shell's foreground process group). Elsewhere a named terminal is
/// treated as busy, so a new one is used.
pub(super) fn can_check_busy() -> bool {
    cfg!(unix)
}
