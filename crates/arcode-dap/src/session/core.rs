//! State shared by a session's handle and its tasks.
//!
//! Every adapter connection belongs to a *generation*; a restart without
//! adapter support starts a new one. Updates from older generations are
//! ignored, so a restarted session never reports the old adapter's exit.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};

use super::breakpoints::Book;
use crate::config::DebugConfig;
use crate::connection::Connection;
use crate::error::{DapError, Result};
use crate::event::{DebugEvent, EventSink, SessionState};
use crate::types::{Capabilities, SourceBreakpoint, Thread};

/// How many adapter log lines are kept for failure messages.
const LOG_TAIL: usize = 8;
/// How long a failure report waits for the adapter's last log lines.
const LOG_DRAIN: Duration = Duration::from_millis(500);

/// Where the current generation's connection stands.
#[derive(Clone)]
pub(crate) enum Phase {
    /// Spawning / connecting / `initialize` in flight.
    Starting,
    /// `initialize` answered; ordinary requests may flow.
    Ready(Arc<Connection>),
    /// `initialized` seen and configuration sent; breakpoints may be set.
    Configured(Arc<Connection>),
    /// The session ended (failure, exit or stop).
    Ended(String),
}

pub(crate) struct Data {
    pub(crate) config: DebugConfig,
    pub(crate) generation: u64,
    pub(crate) capabilities: Option<Capabilities>,
    pub(crate) threads: BTreeMap<i64, Thread>,
    pub(crate) conn: Option<Arc<Connection>>,
    /// The end of the current generation was reported.
    pub(crate) ended: bool,
    /// The user asked to stop; the adapter going away is expected.
    pub(crate) stopping: bool,
    /// The last handle is gone: no generation may start any more.
    pub(crate) dropped: bool,
    /// Breakpoint versions and what configuration sent.
    pub(crate) book: Book,
    /// Startup waits on this for the first `initialized` event.
    pub(crate) initialized_tx: Option<oneshot::Sender<()>>,
    pub(crate) log_tail: VecDeque<String>,
    /// Becomes true once all adapter log streams reached EOF.
    pub(crate) logs_done: Option<watch::Receiver<bool>>,
    /// The adapter can be spawned again (false for stream sessions).
    pub(crate) respawnable: bool,
    pub(crate) exit_code: Option<i64>,
}

/// Shared session state.
pub(crate) struct Core {
    /// `None` only if the runtime could not be created.
    pub(crate) runtime: Option<Handle>,
    pub(crate) events: EventSink,
    data: Mutex<Data>,
    state: watch::Sender<SessionState>,
    phase: watch::Sender<Phase>,
}

impl Core {
    pub(crate) fn new(
        runtime: Option<Handle>,
        events: EventSink,
        config: DebugConfig,
        respawnable: bool,
    ) -> Arc<Core> {
        Arc::new(Core {
            runtime,
            events,
            data: Mutex::new(Data {
                config,
                generation: 0,
                capabilities: None,
                threads: BTreeMap::new(),
                conn: None,
                ended: false,
                stopping: false,
                dropped: false,
                book: Book::default(),
                initialized_tx: None,
                log_tail: VecDeque::new(),
                logs_done: None,
                respawnable,
                exit_code: None,
            }),
            state: watch::channel(SessionState::Initializing).0,
            phase: watch::channel(Phase::Starting).0,
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn generation(&self) -> u64 {
        self.lock().generation
    }

    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.generation() == generation
    }

    pub(crate) fn state(&self) -> SessionState {
        self.state.borrow().clone()
    }

    pub(crate) fn subscribe_state(&self) -> watch::Receiver<SessionState> {
        self.state.subscribe()
    }

    /// Sets the state if `generation` is current and `allow(old)` holds;
    /// emits [`DebugEvent::StateChanged`] on change. Returns the old state
    /// if it changed.
    pub(crate) fn transition(
        &self,
        generation: u64,
        allow: impl FnOnce(&SessionState) -> bool,
        new: SessionState,
    ) -> Option<SessionState> {
        let data = self.lock();
        if data.generation != generation {
            return None;
        }
        let mut old = None;
        self.state.send_if_modified(|state| {
            if *state == new || !allow(state) {
                return false;
            }
            old = Some(std::mem::replace(state, new.clone()));
            true
        });
        if old.is_some() {
            // Emitted under the data lock so state events are totally ordered.
            self.events.send(DebugEvent::StateChanged(new));
        }
        drop(data);
        old
    }

    /// Unconditional [`Core::transition`].
    pub(crate) fn set_state(&self, generation: u64, new: SessionState) {
        self.transition(generation, |_| true, new);
    }

    pub(crate) fn set_phase(&self, generation: u64, phase: Phase) {
        let data = self.lock();
        if data.generation == generation {
            self.phase.send_replace(phase);
        }
    }

    /// Starts a new generation (restart): resets per-adapter state and
    /// clears `stopping`. `None` once the session was dropped.
    pub(crate) fn next_generation(&self) -> Option<u64> {
        let mut data = self.lock();
        if data.dropped {
            return None;
        }
        data.generation += 1;
        data.stopping = false;
        data.book.reset_sent();
        data.conn = None;
        data.ended = false;
        data.initialized_tx = None;
        data.threads.clear();
        data.log_tail.clear();
        data.logs_done = None;
        data.exit_code = None;
        self.phase.send_replace(Phase::Starting);
        Some(data.generation)
    }

    /// Waits until requests can be sent (`configured`: until breakpoints
    /// may be set) and returns the connection.
    pub(crate) async fn connection(&self, configured: bool) -> Result<Arc<Connection>> {
        let mut rx = self.phase.subscribe();
        let phase = rx
            .wait_for(|p| match p {
                Phase::Starting => false,
                Phase::Ready(_) => !configured,
                Phase::Configured(_) | Phase::Ended(_) => true,
            })
            .await
            .map_err(|_| DapError::SessionEnded("session dropped".into()))?
            .clone();
        match phase {
            Phase::Ready(c) | Phase::Configured(c) => Ok(c),
            Phase::Ended(reason) => Err(DapError::SessionEnded(reason)),
            Phase::Starting => Err(DapError::SessionEnded("not started".into())),
        }
    }

    /// The current connection without waiting, if any.
    pub(crate) fn current_connection(&self) -> Option<Arc<Connection>> {
        self.lock().conn.clone()
    }

    pub(crate) fn capabilities(&self) -> Capabilities {
        self.lock().capabilities.clone().unwrap_or_default()
    }

    pub(crate) fn breakpoints(&self) -> BTreeMap<PathBuf, Vec<SourceBreakpoint>> {
        self.lock().config.breakpoints.clone()
    }

    /// Records an adapter log line and forwards it.
    pub(crate) fn log_line(&self, generation: u64, line: String) {
        let mut data = self.lock();
        if data.generation != generation {
            return;
        }
        if data.log_tail.len() == LOG_TAIL {
            data.log_tail.pop_front();
        }
        data.log_tail.push_back(line.clone());
        self.events.send(DebugEvent::AdapterLog(line));
    }

    /// Ends `generation` once: phase `Ended`, state `Terminated`, then
    /// `event` (built from the session data, e.g. the adapter's recent log
    /// lines and whether a stop was requested), so the UI sees
    /// a consistent handle when the event arrives. Returns the connection
    /// to dispose of; `None` if the generation already ended or is stale.
    pub(crate) fn end_generation(
        &self,
        generation: u64,
        reason: String,
        event: impl FnOnce(&Data) -> DebugEvent,
    ) -> Option<Option<Arc<Connection>>> {
        let (event, conn) = {
            let mut data = self.lock();
            if data.generation != generation || data.ended {
                return None;
            }
            data.ended = true;
            self.phase.send_replace(Phase::Ended(reason));
            (event(&data), data.conn.take())
        };
        self.set_state(generation, SessionState::Terminated);
        self.events.send(event);
        Some(conn)
    }

    /// Ends the generation with [`DebugEvent::AdapterFailed`] (once, after
    /// waiting briefly for the adapter's last log lines) and kills it. A
    /// failure caused by a requested stop is reported as
    /// [`DebugEvent::AdapterExited`] instead.
    pub(crate) async fn fail(&self, generation: u64, message: String) {
        self.wait_logs(generation).await;
        let ended = self.end_generation(generation, message.clone(), |data| {
            if data.stopping {
                return DebugEvent::AdapterExited(message);
            }
            let message = with_tail(message, &data.log_tail);
            log::warn!("debug session failed: {message}");
            DebugEvent::AdapterFailed(message)
        });
        if let Some(Some(conn)) = ended {
            conn.kill("session failed");
        }
    }

    /// Synchronous failure report (no log draining, no runtime needed).
    pub(crate) fn fail_now(&self, generation: u64, message: String) {
        self.end_generation(generation, message.clone(), |_| {
            DebugEvent::AdapterFailed(message)
        });
    }

    /// The connection of `generation` closed.
    pub(crate) async fn on_closed(&self, generation: u64, reason: String) {
        let starting = self.state() == SessionState::Initializing;
        let stopping = self.lock().stopping;
        if starting && !stopping {
            return self.fail(generation, reason).await;
        }
        self.end_generation(generation, reason.clone(), |_| {
            DebugEvent::AdapterExited(reason)
        });
    }

    /// Waits briefly for the adapter's log streams to reach EOF.
    async fn wait_logs(&self, generation: u64) {
        let rx = {
            let data = self.lock();
            if data.generation != generation {
                return;
            }
            data.logs_done.clone()
        };
        if let Some(mut rx) = rx {
            let _ = tokio::time::timeout(LOG_DRAIN, rx.wait_for(|done| *done)).await;
        }
    }
}

fn with_tail(message: String, tail: &VecDeque<String>) -> String {
    let lines: Vec<&str> = tail
        .iter()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() || lines.iter().all(|l| message.contains(l)) {
        message
    } else {
        format!("{message}\nadapter output:\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_is_appended_once() {
        let tail: VecDeque<String> = ["", "No module named debugpy"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            with_tail("adapter exited".into(), &tail),
            "adapter exited\nadapter output:\nNo module named debugpy"
        );
        assert_eq!(with_tail("x".into(), &VecDeque::new()), "x");
        assert_eq!(
            with_tail("boom: No module named debugpy".into(), &tail),
            "boom: No module named debugpy"
        );
    }
}
