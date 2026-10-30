//! [`DebugSession`]: starting a session, and the UI-facing
//! [`DebugSessionHandle`].

mod breakpoints;
mod control;
mod core;
mod execution;
mod inbound;
mod queue;
mod runtime;
mod startup;

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::oneshot;

pub use control::STOP_TIMEOUT;
use core::Core;
use queue::Queue;
pub use startup::{INITIALIZE_TIMEOUT, REQUEST_TIMEOUT};

use crate::config::DebugConfig;
use crate::connection::Connection;
use crate::error::{DapError, Result};
use crate::event::{EventReceiver, EventSink, SessionState};
use crate::process::{BoxReader, BoxWriter};
use crate::types::{
    Breakpoint, Capabilities, EvaluateArguments, EvaluateContext, EvaluateResponseBody,
    FunctionBreakpoint, Scope, ScopesArguments, ScopesResponseBody, SourceBreakpoint, StackFrame,
    StackTraceArguments, StackTraceResponseBody, Thread, ThreadsResponseBody, Variable,
    VariablesArguments, VariablesResponseBody,
};

/// Entry point: starts debug sessions on the crate's own tokio runtime.
pub struct DebugSession;

impl DebugSession {
    /// Starts a session: spawns/connects the adapter and runs `initialize`
    /// → `launch`/`attach` → (`initialized`) → breakpoints →
    /// `configurationDone` in the background. Never blocks; failures arrive
    /// as [`crate::DebugEvent::AdapterFailed`].
    pub fn start(config: DebugConfig) -> (DebugSessionHandle, EventReceiver) {
        Self::spawn(config, None)
    }

    /// Like [`DebugSession::start`] over an already-connected byte stream
    /// (an in-process adapter, a forwarded socket, tests). The config's
    /// transport is not used, so restart needs the adapter's `restart`.
    pub fn start_with_streams<R, W>(
        config: DebugConfig,
        reader: R,
        writer: W,
    ) -> (DebugSessionHandle, EventReceiver)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::spawn(
            config,
            Some((Box::new(BufReader::new(reader)), Box::new(writer))),
        )
    }

    fn spawn(
        config: DebugConfig,
        streams: Option<(BoxReader, BoxWriter)>,
    ) -> (DebugSessionHandle, EventReceiver) {
        let (events, receiver) = EventSink::channel();
        let respawnable = streams.is_none();
        match runtime::handle() {
            Ok(rt) => {
                let core = Core::new(Some(rt.clone()), events, config, respawnable);
                rt.spawn(startup::run(core.clone(), 0, streams));
                (DebugSessionHandle::new(core), receiver)
            }
            Err(e) => {
                let core = Core::new(None, events, config, false);
                core.fail_now(0, e.to_string());
                (DebugSessionHandle::new(core), receiver)
            }
        }
    }
}

/// Kills the adapter when the last handle clone is dropped.
struct Lifetime {
    core: Arc<Core>,
    queue: Queue,
}

impl Drop for Lifetime {
    fn drop(&mut self) {
        control::finish_dropped(&self.core);
    }
}

/// A cheap-to-clone handle to one debug session. Every method returns
/// immediately; request futures are runtime-agnostic (await them on any
/// executor), are sent even if never polled, and fail instead of hanging if
/// the adapter dies. Requests reach the adapter in call order (across all
/// clones of a handle called from one thread). Dropping the last clone
/// kills the adapter.
#[derive(Clone)]
pub struct DebugSessionHandle {
    lifetime: Arc<Lifetime>,
}

impl std::fmt::Debug for DebugSessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugSessionHandle")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl DebugSessionHandle {
    fn new(core: Arc<Core>) -> Self {
        let queue = Queue::spawn(&core);
        DebugSessionHandle {
            lifetime: Arc::new(Lifetime { core, queue }),
        }
    }

    fn core(&self) -> &Arc<Core> {
        &self.lifetime.core
    }

    /// Current client-side state.
    pub fn state(&self) -> SessionState {
        self.core().state()
    }

    /// Adapter capabilities (default until `initialize` is answered).
    pub fn capabilities(&self) -> Capabilities {
        self.core().capabilities()
    }

    /// Threads known from `thread` events and the last [`Self::threads`] call.
    pub fn known_threads(&self) -> Vec<Thread> {
        self.core().lock().threads.values().cloned().collect()
    }

    /// The breakpoints the session keeps sending (per file).
    pub fn breakpoints(&self) -> BTreeMap<PathBuf, Vec<SourceBreakpoint>> {
        self.core().breakpoints()
    }

    /// Exit code from the `exited` event, once known.
    pub fn exit_code(&self) -> Option<i64> {
        self.core().lock().exit_code
    }

    /// Queues `work` behind all earlier requests: once the session is ready
    /// (`configured`: configured), `work` runs on the dispatcher, must
    /// enqueue its requests synchronously and returns a future for the
    /// result, which runs on the session runtime.
    fn submit<T, F>(
        &self,
        configured: bool,
        work: impl FnOnce(&Arc<Core>, Arc<Connection>) -> F + Send + 'static,
    ) -> impl Future<Output = Result<T>> + Send + 'static
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job = move |core: &Arc<Core>, conn: Result<Arc<Connection>>| match conn {
            Ok(conn) => {
                let fut = work(core, conn);
                // The dispatcher runs inside the session runtime.
                drop(tokio::spawn(async move {
                    let _ = tx.send(fut.await);
                }));
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        };
        self.lifetime
            .queue
            .push(self.core(), configured, Box::new(job));
        async move {
            rx.await
                .unwrap_or_else(|_| Err(DapError::SessionEnded("runtime shut down".into())))
        }
    }

    /// Runs `work` on the session runtime, outside the request queue; the
    /// returned future only waits for its result.
    fn spawn<T, F>(
        &self,
        work: impl FnOnce(Arc<Core>) -> F,
    ) -> impl Future<Output = Result<T>> + Send + 'static
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let fut = work(self.core().clone());
        match &self.core().runtime {
            Some(rt) => drop(rt.spawn(async move {
                let _ = tx.send(fut.await);
            })),
            None => drop(tx),
        }
        async move {
            rx.await
                .unwrap_or_else(|_| Err(DapError::SessionEnded("runtime shut down".into())))
        }
    }

    /// Sends any request once the adapter is initialized and returns the
    /// raw response body.
    pub fn request(
        &self,
        command: impl Into<String>,
        arguments: Option<Value>,
    ) -> impl Future<Output = Result<Option<Value>>> + Send + 'static {
        let command = command.into();
        self.submit(false, move |_, conn| {
            conn.request(&command, arguments, Some(REQUEST_TIMEOUT))
        })
    }

    fn typed<A, T>(
        &self,
        command: &'static str,
        args: A,
    ) -> impl Future<Output = Result<T>> + Send + 'static
    where
        A: Serialize,
        T: DeserializeOwned + Default + Send + 'static,
    {
        let args = serde_json::to_value(args).map_err(DapError::from);
        let fut = args.map(|a| self.request(command, Some(a)));
        async move { parse_body(fut?.await?) }
    }

    /// Replaces the breakpoints of `path` (empty: remove all) and returns
    /// the adapter's view. Before configuration they are only stored and
    /// sent once, with the startup sequence (the future resolves with that
    /// answer).
    pub fn set_breakpoints(
        &self,
        path: impl Into<PathBuf>,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + 'static {
        let path = path.into();
        let version = {
            let mut data = self.core().lock();
            if breakpoints.is_empty() {
                data.config.breakpoints.remove(&path);
            } else {
                data.config
                    .breakpoints
                    .insert(path.clone(), breakpoints.clone());
            }
            data.book.bump(Some(path.clone()))
        };
        self.submit(true, move |core, conn| {
            let sent = core.lock().book.sent_since(&Some(path.clone()), version);
            let request = match sent {
                Some(answer) => Err(answer),
                None => Ok(breakpoints::send(
                    core.clone(),
                    core.generation(),
                    &conn,
                    path,
                    breakpoints,
                )),
            };
            async move {
                match request {
                    Ok(request) => request.await,
                    Err(answer) => answer,
                }
            }
        })
    }

    /// Replaces all function breakpoints (like [`Self::set_breakpoints`]).
    pub fn set_function_breakpoints(
        &self,
        breakpoints: Vec<FunctionBreakpoint>,
    ) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + 'static {
        let version = {
            let mut data = self.core().lock();
            data.config.function_breakpoints = breakpoints.clone();
            data.book.bump(None)
        };
        self.submit(true, move |core, conn| {
            let request = if !core.capabilities().supports_function_breakpoints {
                Err(Err(DapError::Unsupported("function breakpoints".into())))
            } else if let Some(answer) = core.lock().book.sent_since(&None, version) {
                Err(answer)
            } else {
                Ok(breakpoints::send_functions(&conn, breakpoints))
            };
            async move {
                match request {
                    Ok(request) => request.await,
                    Err(answer) => answer,
                }
            }
        })
    }

    /// Lists threads (and refreshes [`Self::known_threads`]).
    pub fn threads(&self) -> impl Future<Output = Result<Vec<Thread>>> + Send + 'static {
        self.submit(false, move |core, conn| {
            let request = conn.request("threads", None, Some(REQUEST_TIMEOUT));
            let (core, generation) = (core.clone(), core.generation());
            async move {
                let threads = parse_body::<ThreadsResponseBody>(request.await?)?.threads;
                let mut data = core.lock();
                if data.generation == generation {
                    data.threads = threads.iter().map(|t| (t.id, t.clone())).collect();
                }
                Ok(threads)
            }
        })
    }

    /// Stack frames of a stopped thread, innermost first.
    pub fn stack_trace(
        &self,
        thread_id: i64,
    ) -> impl Future<Output = Result<Vec<StackFrame>>> + Send + 'static {
        let args = StackTraceArguments {
            thread_id,
            start_frame: None,
            levels: None,
        };
        let fut = self.typed::<_, StackTraceResponseBody>("stackTrace", args);
        async move { Ok(fut.await?.stack_frames) }
    }

    /// Scopes of a frame.
    pub fn scopes(
        &self,
        frame_id: i64,
    ) -> impl Future<Output = Result<Vec<Scope>>> + Send + 'static {
        let fut = self.typed::<_, ScopesResponseBody>("scopes", ScopesArguments { frame_id });
        async move { Ok(fut.await?.scopes) }
    }

    /// Children of a scope or structured variable.
    pub fn variables(
        &self,
        variables_reference: i64,
    ) -> impl Future<Output = Result<Vec<Variable>>> + Send + 'static {
        let args = VariablesArguments {
            variables_reference,
        };
        let fut = self.typed::<_, VariablesResponseBody>("variables", args);
        async move { Ok(fut.await?.variables) }
    }

    /// Evaluates an expression (in a frame, or globally).
    pub fn evaluate(
        &self,
        expression: impl Into<String>,
        frame_id: Option<i64>,
        context: EvaluateContext,
    ) -> impl Future<Output = Result<EvaluateResponseBody>> + Send + 'static {
        let args = EvaluateArguments {
            expression: expression.into(),
            frame_id,
            context: Some(context),
        };
        self.typed("evaluate", args)
    }
}

fn parse_body<T: DeserializeOwned + Default>(body: Option<Value>) -> Result<T> {
    match body {
        None | Some(Value::Null) => Ok(T::default()),
        Some(v) => serde_json::from_value(v).map_err(DapError::from),
    }
}
