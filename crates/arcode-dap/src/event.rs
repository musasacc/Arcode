//! Events delivered to the UI, the session state and the `runInTerminal`
//! responder.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use crate::connection::Connection;
use crate::error::{DapError, Result};
use crate::types::{
    Breakpoint, BreakpointEventBody, Capabilities, ContinuedEventBody, ExitedEventBody,
    OutputEventBody, ProcessEventBody, RunInTerminalRequestArguments, RunInTerminalResponseBody,
    StoppedEventBody, TerminatedEventBody, ThreadEventBody,
};

/// Lifecycle of a debug session as tracked by the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Adapter starting: initialize, launch/attach, configuration.
    Initializing,
    /// The debuggee runs.
    Running,
    /// The debuggee is paused.
    Stopped {
        /// Thread that stopped (focus it in the UI).
        thread_id: Option<i64>,
        /// `"breakpoint"`, `"step"`, `"exception"`, `"pause"`, `"entry"`, ...
        reason: String,
    },
    /// Debugging ended (debuggee exited, adapter gone or stopped).
    Terminated,
}

impl SessionState {
    /// True for [`SessionState::Stopped`].
    pub fn is_stopped(&self) -> bool {
        matches!(self, SessionState::Stopped { .. })
    }
}

/// Something the UI may want to react to. Adapter events are forwarded in
/// wire order; [`DebugEvent::StateChanged`] follows the event that caused it.
#[derive(Debug, Clone)]
pub enum DebugEvent {
    /// The client-side state changed.
    StateChanged(SessionState),
    /// Capabilities from `initialize` (and later `capabilities` events, merged).
    Capabilities(Capabilities),
    /// The adapter sent `initialized`; configuration follows automatically.
    Initialized,
    /// `stopped`.
    Stopped(StoppedEventBody),
    /// `continued`.
    Continued(ContinuedEventBody),
    /// `exited`: the debuggee exited with a code.
    Exited(ExitedEventBody),
    /// `terminated`: debugging ended; the client disconnects automatically.
    Terminated(TerminatedEventBody),
    /// `output` (program stdout/stderr, adapter console messages).
    Output(OutputEventBody),
    /// `breakpoint`: the adapter changed, added or removed a breakpoint.
    Breakpoint(BreakpointEventBody),
    /// Result of a `setBreakpoints` for one file (startup or
    /// [`crate::DebugSessionHandle::set_breakpoints`]).
    BreakpointsSet {
        /// The file.
        path: PathBuf,
        /// Adapter's view of the file's breakpoints, in request order.
        breakpoints: Vec<Breakpoint>,
    },
    /// `thread`: a thread started or exited.
    Thread(ThreadEventBody),
    /// `process`: the debuggee process is known.
    Process(ProcessEventBody),
    /// The adapter asks the UI to run the debuggee in a terminal. Answer
    /// with [`RunInTerminalRequest::respond`] (or `fail`); dropping every
    /// clone unanswered replies with an error.
    RunInTerminal(RunInTerminalRequest),
    /// A line the adapter process wrote to stderr (or stdout for TCP adapters).
    AdapterLog(String),
    /// The adapter exited or closed the connection after a successful start.
    AdapterExited(String),
    /// The session could not start, or the adapter died while starting.
    AdapterFailed(String),
}

/// Runtime-agnostic receiving end of a session's events (poll it from any
/// executor, e.g. a GPUI task). Closes when the session is gone.
pub type EventReceiver = async_channel::Receiver<DebugEvent>;

/// Sending end of the event channel; never blocks.
#[derive(Debug, Clone)]
pub(crate) struct EventSink(async_channel::Sender<DebugEvent>);

impl EventSink {
    pub(crate) fn channel() -> (EventSink, EventReceiver) {
        let (tx, rx) = async_channel::unbounded();
        (EventSink(tx), rx)
    }

    /// Sends an event; drops it when the receiver is gone.
    pub(crate) fn send(&self, event: DebugEvent) {
        let _ = self.0.try_send(event);
    }
}

/// A `runInTerminal` reverse request waiting for the UI's answer.
#[derive(Debug, Clone)]
pub struct RunInTerminalRequest {
    /// What to run: `args[0]` is the program, run in `cwd` with `env` changes.
    pub arguments: RunInTerminalRequestArguments,
    responder: Arc<Responder>,
}

#[derive(Debug)]
struct Responder {
    conn: Weak<Connection>,
    request_seq: i64,
    answered: AtomicBool,
}

impl Responder {
    fn answer(&self, result: std::result::Result<RunInTerminalResponseBody, String>) -> Result<()> {
        if self.answered.swap(true, Ordering::SeqCst) {
            return Err(DapError::Protocol("runInTerminal was already answered".into()));
        }
        let conn = self
            .conn
            .upgrade()
            .ok_or_else(|| DapError::SessionEnded("adapter connection closed".into()))?;
        let body = match result {
            Ok(body) => Ok(Some(serde_json::to_value(body)?)),
            Err(e) => Err(e),
        };
        conn.respond(self.request_seq, "runInTerminal", body)
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        if !self.answered.load(Ordering::SeqCst) {
            let _ = self.answer(Err("runInTerminal was not handled by the client".into()));
        }
    }
}

impl RunInTerminalRequest {
    pub(crate) fn new(
        arguments: RunInTerminalRequestArguments,
        conn: &Arc<Connection>,
        request_seq: i64,
    ) -> Self {
        RunInTerminalRequest {
            arguments,
            responder: Arc::new(Responder {
                conn: Arc::downgrade(conn),
                request_seq,
                answered: AtomicBool::new(false),
            }),
        }
    }

    /// Reports that the program was started (`process_id` of the program,
    /// `shell_process_id` of the terminal's shell, when known).
    pub fn respond(&self, process_id: Option<u32>, shell_process_id: Option<u32>) -> Result<()> {
        self.responder.answer(Ok(RunInTerminalResponseBody {
            process_id,
            shell_process_id,
        }))
    }

    /// Reports that the program could not be started.
    pub fn fail(&self, message: impl Into<String>) -> Result<()> {
        self.responder.answer(Err(message.into()))
    }

    /// True once answered (by any clone).
    pub fn is_answered(&self) -> bool {
        self.responder.answered.load(Ordering::SeqCst)
    }
}
