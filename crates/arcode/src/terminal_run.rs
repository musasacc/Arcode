//! The run readout (docs/DESIGN.md 6.8, 6.10): the last command started in
//! a named terminal (Run, a run button, a package command), whether it is
//! still running, its exit code and how long it took.
//!
//! A command typed into a shell does not report its exit code, so for
//! those the end is found by watching the shell's foreground process
//! group (Unix only; a cheap `/proc` read on Linux), polled often at first
//! and rarely later, and only while the command runs. Programs started
//! without a shell (package commands) report their exit code.
//!
//! The running time ticks without redrawing more than it must: whole
//! seconds for the first minute, then whole minutes (minutes only in
//! low-end mode), and only while the readout is on screen. A dev server
//! started with Run thus costs one frame a minute, not one a second.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use arcode_terminal_view::TerminalView;
use arcode_ui_kit::metrics::type_scale;
use arcode_ui_kit::{Tooltip, low_end, mono_font_family, status_dot, theme, truncated};
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
    /// The readout text last drawn, taken by the watcher when it asks for
    /// a redraw: `None` while the readout is not on screen, so a hidden
    /// readout never makes the window redraw.
    drawn: RefCell<Option<String>>,
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
    /// `running · 4 s`, `done · 0.31 s`; `coarse` (low-end mode) counts a
    /// running command in whole minutes only.
    pub fn readout(&self, coarse: bool) -> String {
        readout_text(
            self.status(),
            self.exit.flatten(),
            self.duration,
            self.started.elapsed(),
            self.watched,
            coarse,
        )
    }

    /// The readout text to draw now, remembered as drawn (see
    /// [`LastRun::stale`]). Every view that shows the readout (the bottom
    /// panel, the footer while the panel is hidden) draws this text.
    pub fn shown_readout(&self, cx: &App) -> String {
        let text = self.readout(low_end(cx));
        *self.drawn.borrow_mut() = Some(text.clone());
        text
    }

    /// Whether the readout on screen is out of date: it was drawn (and is
    /// therefore shown) with another text than `now`. Forgets the drawn
    /// text, so a readout that is not drawn again is not asked for again.
    fn stale(&self, now: &str) -> bool {
        let mut drawn = self.drawn.borrow_mut();
        match drawn.as_deref() {
            Some(shown) if shown != now => {
                *drawn = None;
                true
            }
            _ => false,
        }
    }
}

/// The readout text for a run in `status` (see [`LastRun::readout`]).
pub fn readout_text(
    status: RunStatus,
    code: Option<i32>,
    duration: Option<Duration>,
    elapsed: Duration,
    watched: bool,
    coarse: bool,
) -> String {
    let with = |head: String, d: Option<Duration>| match d {
        Some(d) => format!("{head} · {}", format_duration(d)),
        None => head,
    };
    match status {
        RunStatus::Running => running_text(elapsed, coarse),
        RunStatus::Succeeded | RunStatus::Failed => match code {
            Some(code) => with(format!("exit {code}"), duration),
            None => with("stopped".to_string(), duration),
        },
        RunStatus::Ended if !watched => "started".to_string(),
        RunStatus::Ended => with("done".to_string(), duration),
    }
}

/// The readout of a command running for `elapsed`: `running · 4 s` for
/// the first minute, then `running · 2 m`. `coarse` (low-end mode) drops
/// the seconds: `running`, then `running · 1 m`. The text changes at most
/// once a second, and once a minute after the first.
pub fn running_text(elapsed: Duration, coarse: bool) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..60 if coarse => "running".to_string(),
        0..60 => format!("running · {secs} s"),
        _ => format!("running · {} m", secs / 60),
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
            drawn: RefCell::new(None),
            _watch: watch,
        });
        cx.notify();
    }

    /// The terminal of the last run exited with `code`: the run ended.
    pub(super) fn run_terminal_exited(&mut self, id: EntityId, code: Option<i32>) {
        if let Some(run) = self
            .last_run
            .as_mut()
            .filter(|r| r.terminal == id && !r.ended)
        {
            run.ended = true;
            run.watched = true;
            run.exit = Some(code);
            run.duration = Some(run.started.elapsed());
            run._watch = None;
        }
    }

    /// Polls whether the shell `pid` still runs a command in the
    /// foreground; ends the run when it went back to the prompt. Asks for
    /// a redraw while it runs only when the readout on screen is out of
    /// date (see [`LastRun::stale`]).
    fn watch_shell(&self, pid: u32, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let mut seen_busy = false;
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
                        if run.stale(&run.readout(low_end(cx))) {
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
/// the mono readout, both ending in `…` when the header is short of room.
/// `None` before the first run.
pub fn render_run_readout(panel: &Entity<TerminalPanel>, cx: &App) -> Option<AnyElement> {
    let run = panel.read(cx).last_run.as_ref()?;
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let text = run.shown_readout(cx);
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
            .overflow_hidden()
            .text_size(scale.meta)
            .child(status_dot(dot))
            .child(
                div()
                    .flex()
                    .min_w(px(24.))
                    .max_w(px(200.))
                    .overflow_hidden()
                    .text_color(c.text)
                    .child(truncated(run.command.clone())),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .overflow_hidden()
                    .font_family(mono_font_family())
                    .text_size(scale.readout)
                    .text_color(c.text_muted)
                    .child(truncated(text)),
            )
            .tooltip(move |_, cx| Tooltip::text(tip.clone(), cx))
            .into_any_element(),
    )
}

#[cfg(test)]
#[path = "terminal_run_tests.rs"]
mod tests;
