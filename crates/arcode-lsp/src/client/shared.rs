//! State shared between a client handle and its reader/writer tasks:
//! the outgoing queue, pending requests and liveness.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};

use crate::error::{LspError, Result};
use crate::event::{EventSink, LspEvent, PositionEncoding, ServerStatus};
use crate::rpc;

type PendingSender = oneshot::Sender<Result<Value>>;

struct State {
    alive: bool,
    reason: String,
    pending: HashMap<i64, PendingSender>,
    /// `initialize` finished; status changes may be reported.
    ready: bool,
    /// Title of the work-done progress shown as `Busy`, if any.
    busy: Option<String>,
}

/// Connection state for one server.
pub(crate) struct Shared {
    pub(crate) name: String,
    pub(crate) root: PathBuf,
    pub(crate) events: EventSink,
    pub(crate) initialization_options: Option<Value>,
    /// Negotiated position encoding, set once `initialize` answered.
    encoding: OnceLock<PositionEncoding>,
    outgoing: mpsc::UnboundedSender<String>,
    state: Mutex<State>,
    next_id: AtomicI64,
    shutting_down: AtomicBool,
    terminated: watch::Sender<bool>,
}

impl Shared {
    /// Creates the shared state and the receiving end of the outgoing queue.
    pub(crate) fn new(
        name: String,
        root: PathBuf,
        events: EventSink,
        initialization_options: Option<Value>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            name,
            root,
            events,
            initialization_options,
            encoding: OnceLock::new(),
            outgoing: tx,
            state: Mutex::new(State {
                alive: true,
                reason: String::new(),
                pending: HashMap::new(),
                ready: false,
                busy: None,
            }),
            next_id: AtomicI64::new(1),
            shutting_down: AtomicBool::new(false),
            terminated: watch::channel(false).0,
        });
        (shared, rx)
    }

    /// Records the encoding negotiated in `initialize`.
    pub(crate) fn set_encoding(&self, encoding: PositionEncoding) {
        let _ = self.encoding.set(encoding);
    }

    /// The negotiated encoding (UTF-16 before `initialize` answered).
    pub(crate) fn encoding(&self) -> PositionEncoding {
        self.encoding.get().copied().unwrap_or_default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True until the connection terminates.
    pub(crate) fn is_alive(&self) -> bool {
        self.lock().alive
    }

    /// The error requests fail with once the connection has terminated.
    pub(crate) fn stopped_error(&self) -> LspError {
        let reason = self.lock().reason.clone();
        LspError::ServerStopped(if reason.is_empty() {
            "not running".into()
        } else {
            reason
        })
    }

    /// Queues a raw message for the writer task.
    pub(crate) fn send_raw(&self, message: String) -> Result<()> {
        if !self.is_alive() {
            return Err(self.stopped_error());
        }
        self.outgoing
            .send(message)
            .map_err(|_| LspError::ServerStopped("writer closed".into()))
    }

    /// Sends a notification.
    pub(crate) fn notify_value(&self, method: &str, params: Value) -> Result<()> {
        self.send_raw(rpc::notification(method, params))
    }

    /// Records the work-done progress to show (`None`: idle). Reports
    /// `Busy`/`Running` only after [`Shared::mark_ready`], so progress that
    /// starts during `initialize` is not masked by a later `Running`.
    pub(crate) fn set_busy(&self, title: Option<String>) {
        let mut state = self.lock();
        if state.busy == title {
            return;
        }
        state.busy = title;
        if state.alive && state.ready {
            // Emitted under the lock so status events are totally ordered.
            self.emit_status(ready_status(&state.busy));
        }
    }

    /// Marks initialization complete and reports `Running` (or `Busy` if
    /// progress is already active). Returns false, reporting nothing, if
    /// the connection has already terminated.
    pub(crate) fn mark_ready(&self) -> bool {
        let mut state = self.lock();
        if !state.alive {
            return false;
        }
        state.ready = true;
        self.emit_status(ready_status(&state.busy));
        true
    }

    /// Emits a status event for this server.
    pub(crate) fn emit_status(&self, status: ServerStatus) {
        self.events.send(LspEvent::ServerStatus {
            server: self.name.clone(),
            root: self.root.clone(),
            status,
        });
    }

    /// Emits a log event for this server.
    pub(crate) fn log(&self, level: lsp_types::MessageType, message: String) {
        self.events.send(LspEvent::Log {
            server: self.name.clone(),
            level,
            message,
        });
    }

    /// Sends a request now and returns a future for its result.
    ///
    /// The request is written eagerly, so ordering relative to other
    /// messages is the call order. Dropping the future before the answer
    /// arrives (or hitting `timeout`) sends `$/cancelRequest`. The timeout is
    /// only applied when the future is polled inside a tokio runtime.
    pub(crate) fn request_value(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Value>> + Send + 'static {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let sent = self.register(id, tx).and_then(|()| {
            self.send_raw(rpc::request(id, method, params))
                .inspect_err(|_| {
                    self.lock().pending.remove(&id);
                })
        });
        let guard = CancelGuard {
            shared: Arc::clone(self),
            id,
            armed: sent.is_ok(),
        };
        async move {
            sent?;
            let in_tokio = tokio::runtime::Handle::try_current().is_ok();
            let answer = match timeout {
                Some(limit) if in_tokio => match tokio::time::timeout(limit, rx).await {
                    Ok(answer) => answer,
                    Err(_) => return Err(LspError::Timeout(limit)),
                },
                _ => rx.await,
            };
            guard.disarm();
            answer.unwrap_or_else(|_| Err(LspError::ServerStopped("connection closed".into())))
        }
    }

    fn register(&self, id: i64, tx: PendingSender) -> Result<()> {
        let mut state = self.lock();
        if !state.alive {
            drop(state);
            return Err(self.stopped_error());
        }
        state.pending.insert(id, tx);
        Ok(())
    }

    /// Delivers a response to the matching pending request, if any.
    pub(crate) fn complete(&self, id: i64, result: Result<Value>) {
        let tx = self.lock().pending.remove(&id);
        match tx {
            Some(tx) => {
                let _ = tx.send(result);
            }
            None => log::debug!("[{}] response for unknown request id {id}", self.name),
        }
    }

    /// Marks that a shutdown was requested, so termination counts as a
    /// normal stop rather than a failure.
    pub(crate) fn mark_shutting_down(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    /// Ends the connection once: fails all pending requests and reports
    /// `Stopped` (after a requested shutdown) or `Failed(reason)`.
    pub(crate) fn terminate(&self, reason: impl Into<String>) {
        let reason = reason.into();
        let drained: Vec<PendingSender> = {
            let mut state = self.lock();
            if !state.alive {
                return;
            }
            state.alive = false;
            state.reason = reason.clone();
            let status = if self.shutting_down.load(Ordering::SeqCst) {
                ServerStatus::Stopped
            } else {
                ServerStatus::Failed(reason.clone())
            };
            // Under the lock: nothing can report `Running`/`Busy` after this.
            self.emit_status(status);
            state.pending.drain().map(|(_, tx)| tx).collect()
        };
        for tx in drained {
            let _ = tx.send(Err(LspError::ServerStopped(reason.clone())));
        }
        self.terminated.send_replace(true);
    }

    /// Resolves once the connection has terminated.
    pub(crate) fn terminated(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.terminated.subscribe();
        async move {
            let _ = rx.wait_for(|t| *t).await;
        }
    }
}

fn ready_status(busy: &Option<String>) -> ServerStatus {
    match busy {
        Some(title) => ServerStatus::Busy(title.clone()),
        None => ServerStatus::Running,
    }
}

/// Removes a pending request and tells the server to cancel it when the
/// request future is dropped early.
struct CancelGuard {
    shared: Arc<Shared>,
    id: i64,
    armed: bool,
}

impl CancelGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let was_pending = self.shared.lock().pending.remove(&self.id).is_some();
        if was_pending {
            let _ = self
                .shared
                .notify_value("$/cancelRequest", json!({ "id": self.id }));
        }
    }
}
