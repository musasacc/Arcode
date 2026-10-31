//! Terminals with a fixed name, started for a run command or a debuggee.

use std::path::PathBuf;

use arcode_terminal_view::TerminalView;
use gpui::{App, AppContext, Context, Entity, SharedString, Window};

use super::TerminalPanel;

/// Called with the terminal a named command runs in (`None` when no
/// terminal could be started).
pub type OnStarted = Box<dyn FnOnce(Option<&Entity<TerminalView>>, &mut App)>;

impl TerminalPanel {
    /// Runs `command` in the terminal tab named `title` (started in `cwd`
    /// if there is none, or if every one is busy running something) and
    /// shows it. Whether a tab is busy is checked in the background (it
    /// can spawn `ps`), so the command may start a moment later;
    /// `on_started` gets the terminal then.
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
        let check = cx.background_spawn(async move {
            pids.into_iter()
                .map(arcode_project::procs::is_running_command)
                .collect::<Vec<bool>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let busy = check.await;
            this.update_in(cx, |panel, window, cx| {
                let idle = candidates
                    .into_iter()
                    .zip(busy)
                    .filter(|(_, busy)| !busy)
                    .map(|((t, _), _)| t)
                    .find(|t| t.read(cx).is_alive() && panel.terminals.contains(t));
                match idle {
                    Some(terminal) => {
                        if let Some(ix) = panel.terminals.iter().position(|t| *t == terminal) {
                            panel.active = ix;
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

/// Whether an idle named terminal can be told from a busy one (Unix:
/// the shell's foreground process group). Elsewhere a named terminal is
/// treated as busy, so a new one is used.
pub(super) fn can_check_busy() -> bool {
    cfg!(unix)
}
