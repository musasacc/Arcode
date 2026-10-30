//! Calls into a plugin and its bounded, coalescing mailbox (§5 "Queue
//! hygiene").
//!
//! - `provide-code-lenses` coalesces per document and `fetch-rows` per
//!   table: a newer call replaces the queued one, which is answered
//!   [`CallError::Superseded`].
//! - The mailbox holds at most `capacity` calls; when full, the oldest
//!   coalescable call is dropped, and if there is none the new call is
//!   refused with [`CallError::Busy`].

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use crate::bindings::editor::DocInfo;
use crate::bindings::guest::CodeLens;
use crate::bindings::ui::{Event, RowPage};
use crate::bindings::{DocId, NodeId, PanelId};
use crate::cancel::{CancelToken, lock};

/// One guest export call.
#[derive(Debug, Clone)]
pub enum Call {
    /// `handle-command` with a manifest-local command id and JSON args.
    Command {
        /// Manifest-local id.
        command: String,
        /// JSON text.
        args: String,
    },
    /// `provide-code-lenses`.
    CodeLenses(DocInfo),
    /// `open-view`: a contributed panel or opener was shown.
    OpenView {
        /// Host-allocated panel id (see `InstanceHandle::allocate_panel`).
        panel: PanelId,
        /// Manifest-local panel or opener id.
        contribution: String,
        /// JSON text, `{"path": ...}` for openers.
        args: String,
    },
    /// `handle-ui-event`.
    UiEvent {
        /// Panel.
        panel: PanelId,
        /// Event.
        event: Event,
    },
    /// `fetch-rows`.
    FetchRows {
        /// Panel.
        panel: PanelId,
        /// Table node.
        table: NodeId,
        /// First row.
        start: u64,
        /// Rows wanted.
        count: u32,
    },
    /// `view-closed`.
    ViewClosed(PanelId),
}

/// Calls of the same key replace each other while queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoalesceKey {
    Lenses(DocId),
    Rows(PanelId, NodeId),
}

impl Call {
    fn coalesce_key(&self) -> Option<CoalesceKey> {
        match self {
            Self::CodeLenses(doc) => Some(CoalesceKey::Lenses(doc.id)),
            Self::FetchRows { panel, table, .. } => Some(CoalesceKey::Rows(*panel, *table)),
            _ => None,
        }
    }

    /// The export's WIT name (for messages).
    pub fn export_name(&self) -> &'static str {
        match self {
            Self::Command { .. } => "handle-command",
            Self::CodeLenses(_) => "provide-code-lenses",
            Self::OpenView { .. } => "open-view",
            Self::UiEvent { .. } => "handle-ui-event",
            Self::FetchRows { .. } => "fetch-rows",
            Self::ViewClosed(_) => "view-closed",
        }
    }
}

/// A successful call's result.
#[derive(Debug, Clone, PartialEq)]
pub enum CallOutput {
    /// The export returned `ok` (or has no result).
    Done,
    /// `provide-code-lenses`.
    Lenses(Vec<CodeLens>),
    /// `fetch-rows`.
    Rows(RowPage),
}

/// Why a call produced no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The plugin returned an error message (shown as plain text).
    Plugin(String),
    /// The guest trapped; the instance was dropped. The text is
    /// user-facing ("exceeded 2 s CPU", "out of memory", ...).
    Trapped(String),
    /// Activation failed (component, import check, `activate`).
    Activation(String),
    /// A newer call of the same kind replaced this one.
    Superseded,
    /// The mailbox is full of calls that cannot be dropped.
    Busy,
    /// Restarting after a trap is backing off.
    Backoff(Duration),
    /// Disabled for the session after repeated traps.
    Disabled,
    /// Code lenses are off for the session after repeated overruns.
    LensesDisabled,
    /// The instance is shutting down.
    Stopped,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plugin(m) => write!(f, "{m}"),
            Self::Trapped(m) => write!(f, "stopped: {m}"),
            Self::Activation(m) => write!(f, "activation failed: {m}"),
            Self::Superseded => f.write_str("superseded by a newer call"),
            Self::Busy => f.write_str("plugin is busy"),
            Self::Backoff(d) => write!(f, "restarting in {} ms", d.as_millis()),
            Self::Disabled => f.write_str("disabled after repeated crashes"),
            Self::LensesDisabled => f.write_str("code lenses disabled after repeated overruns"),
            Self::Stopped => f.write_str("plugin stopped"),
        }
    }
}

/// The answer to one call.
pub type CallResult = Result<CallOutput, CallError>;

/// Unique per process; tasks and replies refer to calls by id.
static NEXT_CALL: AtomicU64 = AtomicU64::new(1);

/// A call plus everything the host stamps on it.
#[derive(Debug)]
pub struct Dispatch {
    /// Process-unique id.
    pub id: u64,
    /// The call.
    pub call: Call,
    /// True for user gestures (§4 "User gesture").
    pub gesture: bool,
    /// The document a lens or command is about (`editor = "context"`).
    pub origin_doc: Option<DocId>,
    /// Cancels this call.
    pub cancel: CancelToken,
    /// Where the result goes.
    pub reply: async_channel::Sender<CallResult>,
}

impl Dispatch {
    /// Stamps `call`; returns it and the receiver the app awaits.
    pub fn new(call: Call, gesture: bool) -> (Self, async_channel::Receiver<CallResult>) {
        let (reply, rx) = async_channel::bounded(1);
        let origin_doc = match &call {
            Call::CodeLenses(doc) => Some(doc.id),
            _ => None,
        };
        let d = Self {
            id: NEXT_CALL.fetch_add(1, Ordering::Relaxed),
            call,
            gesture,
            origin_doc,
            cancel: CancelToken::new(),
            reply,
        };
        (d, rx)
    }

    /// Sends the result (ignored if nobody waits any more).
    pub fn answer(&self, result: CallResult) {
        let _ = self.reply.try_send(result);
    }
}

/// What [`Mailbox::recv_timeout`] returned.
#[derive(Debug)]
pub enum Recv {
    /// A call.
    Call(Box<Dispatch>),
    /// Nothing arrived in time.
    Timeout,
    /// Closed and empty.
    Closed,
}

#[derive(Debug, Default)]
struct State {
    queue: VecDeque<Dispatch>,
    closed: bool,
    /// False while no thread serves the mailbox.
    served: bool,
}

/// The bounded mailbox of one instance thread.
#[derive(Debug)]
pub struct Mailbox {
    state: Mutex<State>,
    ready: Condvar,
    capacity: usize,
}

/// Whether [`Mailbox::push`] needs a new serving thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pushed {
    /// A thread is serving the mailbox.
    Queued,
    /// No thread was serving; the caller must start one.
    NeedsThread,
    /// Refused (answered already).
    Refused,
}

impl Mailbox {
    /// An empty mailbox holding at most `capacity` calls.
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            capacity: capacity.max(1),
        }
    }

    /// Queues a call, coalescing and enforcing the capacity.
    pub fn push(&self, d: Dispatch) -> Pushed {
        let mut st = lock(&self.state);
        if st.closed {
            d.answer(Err(CallError::Stopped));
            return Pushed::Refused;
        }
        if let Some(key) = d.call.coalesce_key()
            && let Some(i) = st.queue.iter().position(|q| q.call.coalesce_key() == Some(key))
            && let Some(old) = st.queue.remove(i)
        {
            old.answer(Err(CallError::Superseded));
        }
        if st.queue.len() >= self.capacity {
            match st.queue.iter().position(|q| q.call.coalesce_key().is_some()) {
                Some(i) => {
                    if let Some(old) = st.queue.remove(i) {
                        old.answer(Err(CallError::Superseded));
                    }
                }
                None => {
                    d.answer(Err(CallError::Busy));
                    return Pushed::Refused;
                }
            }
        }
        st.queue.push_back(d);
        self.ready.notify_one();
        if st.served {
            Pushed::Queued
        } else {
            st.served = true;
            Pushed::NeedsThread
        }
    }

    /// Waits for the next call. Never spins: the thread sleeps on a condvar.
    pub fn recv_timeout(&self, timeout: Duration) -> Recv {
        let st = lock(&self.state);
        let (mut st, _) = self
            .ready
            .wait_timeout_while(st, timeout, |s| s.queue.is_empty() && !s.closed)
            .unwrap_or_else(PoisonError::into_inner);
        match st.queue.pop_front() {
            Some(d) => Recv::Call(Box::new(d)),
            None if st.closed => Recv::Closed,
            None => Recv::Timeout,
        }
    }

    /// The serving thread is about to exit. Returns false (and keeps the
    /// thread) if calls arrived meanwhile.
    pub fn retire(&self) -> bool {
        let mut st = lock(&self.state);
        if st.queue.is_empty() || st.closed {
            st.served = false;
            true
        } else {
            false
        }
    }

    /// Refuses new calls and answers queued ones with
    /// [`CallError::Stopped`].
    pub fn close(&self) {
        let mut st = lock(&self.state);
        st.closed = true;
        for d in st.queue.drain(..) {
            d.answer(Err(CallError::Stopped));
        }
        self.ready.notify_all();
    }

    /// Calls waiting.
    pub fn len(&self) -> usize {
        lock(&self.state).queue.len()
    }

    /// True if no call waits.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
