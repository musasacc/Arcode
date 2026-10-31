//! The run readout (docs/DESIGN.md 6.8, 6.10): the last command started in
//! a named terminal (Run, a run button, a package command), whether it is
//! still running, its exit code and how long it took.
//!
//! A command typed into a shell does not report its exit code, so for
//! those the end is found by watching the shell's foreground process
//! group (Unix only; a cheap `/proc` read on Linux), polled often at first
//! and rarely later, and only while the command runs. Programs started
//! without a shell (package commands) report their exit code.

use std::time::{Duration, Instant};

use arcode_terminal_view::TerminalView;
use arcode_ui_kit::metrics::type_scale;
use arcode_ui_kit::{Tooltip, readout, status_dot, theme, truncated};
use gpui::{AnyElement, App, Context, Entity, EntityId, SharedString, Task, div, prelude::*, px};

use super::TerminalPanel;
use super::named::can_check_busy;

/// How the last run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Still running.
    Running,
    /// Ended with exit code 0.
    Succeeded,
    /// Ended with another exit code, or was killed.
    Failed,
    /// Ended (or was handed to the shell) without a known exit code.
    Ended,
}

/// The last named-terminal run.
pub struct LastRun {
    /// What ran (the run command's label, usually its command line).
    pub command: SharedString,
    started: Instant,
    /// How long it ran, once it ended (`None` when the end is unknown).
    duration: Option<Duration>,
    /// It ended.
    ended: bool,
    /// The exit code, when known; `Some(None)` when it was killed.
    exit: Option<Option<i32>>,
    /// The end can be watched (else the run is only reported as started).
    watched: bool,
    terminal: EntityId,
    _watch: Option<Task<()>>,
}

impl LastRun {
    /// How the run went.
    pub fn status(&self) -> RunStatus {
        match (self.ended || !self.watched, self.exit) {
            (false, _) => RunStatus::Running,
            (true, Some(Some(0))) => RunStatus::Succeeded,
            (true, Some(_)) => RunStatus::Failed,
            (true, None) => RunStatus::Ended,
        }
    }

    /// The mono readout after the command: `exit 101 · 0.92 s`,
    /// `running · 4 s`, `done · 0.31 s`.
    pub fn readout(&self) -> String {
        readout_text(
            self.status(),
            self.exit.flatten(),
            self.duration,
            self.started.elapsed(),
            self.watched,
        )
    }

    /// Whole seconds since the start, to redraw the running readout once a
    /// second.
    fn shown_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

/// The readout text for a run in `status` (see [`LastRun::readout`]).
pub fn readout_text(
    status: RunStatus,
    code: Option<i32>,
    duration: Option<Duration>,
    elapsed: Duration,
    watched: bool,
) -> String {
    let with = |head: String, d: Option<Duration>| match d {
        Some(d) => format!("{head} · {}", format_duration(d)),
        None => head,
    };
    match status {
        RunStatus::Running => format!("running · {} s", elapsed.as_secs()),
        RunStatus::Succeeded | RunStatus::Failed => match code {
            Some(code) => with(format!("exit {code}"), duration),
            None => with("stopped".to_string(), duration),
        },
        RunStatus::Ended if !watched => "started".to_string(),
        RunStatus::Ended => with("done".to_string(), duration),
    }
}

/// A duration as the readouts show it: `0.92 s`, `12.3 s`, `2 m 05 s`.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 10. {
        format!("{secs:.2} s")
    } else if secs < 60. {
        format!("{secs:.1} s")
    } else {
        let whole = d.as_secs();
        format!("{} m {:02} s", whole / 60, whole % 60)
    }
}

/// How long to wait before the next look at a run that started `elapsed`
/// ago: often at first, so short commands get a fair duration, then
/// rarely.
pub fn poll_interval(elapsed: Duration) -> Duration {
    if elapsed < Duration::from_secs(2) {
        Duration::from_millis(100)
    } else if elapsed < Duration::from_secs(10) {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(1)
    }
}

/// A shell never seen busy is taken as done after this long (the command
/// finished between two looks, or never started).
const NEVER_BUSY: Duration = Duration::from_millis(1500);

impl TerminalPanel {
    /// The last named-terminal run, if any.
    pub fn last_run(&self) -> Option<&LastRun> {
        self.last_run.as_ref()
    }

    /// Records that `command` started in `terminal`: typed into its shell
    /// (`program` false, watched through the shell's foreground group) or
    /// as the terminal's own program (its exit ends the run).
    pub(super) fn track_run(
        &mut self,
        command: &str,
        terminal: &Entity<TerminalView>,
        program: bool,
        cx: &mut Context<Self>,
    ) {
        let pid = terminal.read(cx).terminal().child_pid();
        let watched = program || (can_check_busy() && pid.is_some());
        let watch = match pid {
            Some(pid) if watched && !program => Some(self.watch_shell(pid, cx)),
            _ => None,
        };
        self.last_run = Some(LastRun {
            command: SharedString::from(command.to_string()),
            started: Instant::now(),
            duration: None,
            ended: false,
            exit: None,
            watched,
            terminal: terminal.entity_id(),
            _watch: watch,
        });
        cx.notify();
    }

    /// The terminal of the last run exited with `code`: the run ended.
    pub(super) fn run_terminal_exited(&mut self, id: EntityId, code: Option<i32>) {
        if let Some(run) = self.last_run.as_mut().filter(|r| r.terminal == id && !r.ended) {
            run.ended = true;
            run.watched = true;
            run.exit = Some(code);
            run.duration = Some(run.started.elapsed());
            run._watch = None;
        }
    }

    /// Polls whether the shell `pid` still runs a command in the
    /// foreground; ends the run when it went back to the prompt.
    fn watch_shell(&self, pid: u32, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let mut seen_busy = false;
            let mut shown = 0;
            loop {
                cx.background_executor()
                    .timer(poll_interval(started.elapsed()))
                    .await;
                let busy = cx
                    .background_spawn(async move { arcode_project::procs::is_running_command(pid) })
                    .await;
                let elapsed = started.elapsed();
                seen_busy |= busy;
                let done = !busy && (seen_busy || elapsed >= NEVER_BUSY);
                let keep_going = this
                    .update(cx, |panel, cx| {
                        let Some(run) = panel.last_run.as_mut().filter(|r| !r.ended) else {
                            return false;
                        };
                        if done {
                            run.ended = true;
                            run.duration = seen_busy.then_some(elapsed);
                            cx.notify();
                            return false;
                        }
                        if run.shown_seconds() != shown {
                            shown = run.shown_seconds();
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
    }
}

/// The run readout of `panel`'s last run: a status dot (`accent` running,
/// `success` / `error` by exit code, muted when unknown), the command and
/// the mono readout. `None` before the first run.
pub fn render_run_readout(panel: &Entity<TerminalPanel>, cx: &App) -> Option<AnyElement> {
    let run = panel.read(cx).last_run.as_ref()?;
    let c = theme(cx).colors.clone();
    let status = run.status();
    let dot = match status {
        RunStatus::Running => c.accent,
        RunStatus::Succeeded => c.success,
        RunStatus::Failed => c.error,
        RunStatus::Ended => c.text_muted,
    };
    let tip: SharedString = match status {
        RunStatus::Ended if run.watched => format!(
            "{}\nThe shell does not report a command's exit code",
            run.command
        )
        .into(),
        _ => run.command.clone(),
    };
    Some(
        div()
            .id("run-readout")
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .text_size(type_scale(cx).meta)
            .child(status_dot(dot))
            .child(
                div()
                    .min_w(px(24.))
                    .max_w(px(200.))
                    .overflow_hidden()
                    .text_color(c.text)
                    .child(truncated(run.command.clone())),
            )
            .child(div().flex_none().child(readout(run.readout())))
            .tooltip(move |_, cx| Tooltip::text(tip.clone(), cx))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_like_instruments() {
        assert_eq!(format_duration(Duration::from_millis(920)), "0.92 s");
        assert_eq!(format_duration(Duration::from_millis(9_994)), "9.99 s");
        assert_eq!(format_duration(Duration::from_millis(12_340)), "12.3 s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2 m 05 s");
    }

    #[test]
    fn readouts_follow_the_status() {
        let d = Some(Duration::from_millis(920));
        let e = Duration::from_millis(4_100);
        assert_eq!(
            readout_text(RunStatus::Failed, Some(101), d, e, true),
            "exit 101 · 0.92 s"
        );
        assert_eq!(
            readout_text(RunStatus::Succeeded, Some(0), d, e, true),
            "exit 0 · 0.92 s"
        );
        assert_eq!(
            readout_text(RunStatus::Failed, None, d, e, true),
            "stopped · 0.92 s"
        );
        assert_eq!(
            readout_text(RunStatus::Running, None, None, e, true),
            "running · 4 s"
        );
        assert_eq!(
            readout_text(RunStatus::Ended, None, d, e, true),
            "done · 0.92 s"
        );
        assert_eq!(readout_text(RunStatus::Ended, None, None, e, true), "done");
        assert_eq!(
            readout_text(RunStatus::Ended, None, None, e, false),
            "started"
        );
    }

    #[test]
    fn polling_slows_down() {
        assert_eq!(poll_interval(Duration::ZERO), Duration::from_millis(100));
        assert_eq!(
            poll_interval(Duration::from_secs(5)),
            Duration::from_millis(250)
        );
        assert_eq!(poll_interval(Duration::from_secs(60)), Duration::from_secs(1));
    }

    fn run(ended: bool, exit: Option<Option<i32>>, watched: bool) -> LastRun {
        LastRun {
            command: "cargo run".into(),
            started: Instant::now(),
            duration: None,
            ended,
            exit,
            watched,
            terminal: EntityId::from(1u64),
            _watch: None,
        }
    }

    #[test]
    fn status_from_the_exit() {
        assert_eq!(run(false, None, true).status(), RunStatus::Running);
        assert_eq!(run(true, Some(Some(0)), true).status(), RunStatus::Succeeded);
        assert_eq!(run(true, Some(Some(2)), true).status(), RunStatus::Failed);
        assert_eq!(run(true, Some(None), true).status(), RunStatus::Failed);
        assert_eq!(run(true, None, true).status(), RunStatus::Ended);
        // Not watchable (no way to see the end): reported as started.
        assert_eq!(run(false, None, false).status(), RunStatus::Ended);
        assert_eq!(run(false, None, false).readout(), "started");
    }
}
