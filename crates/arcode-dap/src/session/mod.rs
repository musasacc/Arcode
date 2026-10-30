//! [`DebugSession`]: starting a session, and the UI-facing
//! [`DebugSessionHandle`].

mod control;
mod core;
mod inbound;
mod runtime;
mod startup;

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::oneshot;

pub use control::STOP_TIMEOUT;
use core::Core;
pub use startup::{INITIALIZE_TIMEOUT, REQUEST_TIMEOUT};

use crate::config::DebugConfig;
use crate::error::{DapError, Result};
use crate::process::{BoxReader, BoxWriter};
use crate::event::{EventReceiver, EventSink, SessionState};
use crate::types::{
    Breakpoint, Capabilities, ContinueArguments, ContinueResponseBody, EvaluateArguments,
    EvaluateContext, EvaluateResponseBody, FunctionBreakpoint, PauseArguments, Scope,
    ScopesArguments, ScopesResponseBody, SetBreakpointsResponseBody,
    SetFunctionBreakpointsArguments, SourceBreakpoint, StackFrame, StackTraceArguments,
    StackTraceResponseBody, StepArguments, Thread, ThreadsResponseBody, Variable,
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
struct Lifetime(Arc<Core>);

impl Drop for Lifetime {
    fn drop(&mut self) {
        let generation = self.0.generation();
        control::finish_stopped(&self.0, generation, "debug session dropped");
    }
}

/// A cheap-to-clone handle to one debug session. Every method returns
/// immediately; request futures are runtime-agnostic (await them on any
/// executor), are sent even if never polled, and fail instead of hanging if
/// the adapter dies. Dropping the last clone kills the adapter.
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
        DebugSessionHandle {
            lifetime: Arc::new(Lifetime(core)),
        }
    }

    fn core(&self) -> &Arc<Core> {
        &self.lifetime.0
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

    /// Runs `work` on the session runtime; the returned future only waits
    /// for its result.
    fn spawn<T, F>(&self, work: impl FnOnce(Arc<Core>) -> F) -> impl Future<Output = Result<T>> + Send + 'static
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
        self.spawn(move |core| async move {
            let conn = core.connection(false).await?;
            conn.request(&command, arguments, Some(REQUEST_TIMEOUT)).await
        })
    }

    fn typed<A, T>(&self, command: &'static str, args: A) -> impl Future<Output = Result<T>> + Send + 'static
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
    /// sent with the startup sequence (the future resolves after that).
    pub fn set_breakpoints(
        &self,
        path: impl Into<PathBuf>,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + 'static {
        let path = path.into();
        {
            let mut data = self.core().lock();
            if breakpoints.is_empty() {
                data.config.breakpoints.remove(&path);
            } else {
                data.config.breakpoints.insert(path.clone(), breakpoints.clone());
            }
        }
        self.spawn(move |core| async move {
            let conn = core.connection(true).await?;
            let generation = core.generation();
            startup::set_breakpoints(&core, generation, &conn, path, breakpoints).await
        })
    }

    /// Replaces all function breakpoints.
    pub fn set_function_breakpoints(
        &self,
        breakpoints: Vec<FunctionBreakpoint>,
    ) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + 'static {
        self.core().lock().config.function_breakpoints = breakpoints.clone();
        self.spawn(move |core| async move {
            let conn = core.connection(true).await?;
            if !core.capabilities().supports_function_breakpoints {
                return Err(DapError::Unsupported("function breakpoints".into()));
            }
            let args = serde_json::to_value(SetFunctionBreakpointsArguments { breakpoints })?;
            let body = conn
                .request("setFunctionBreakpoints", Some(args), Some(REQUEST_TIMEOUT))
                .await?;
            parse_body::<SetBreakpointsResponseBody>(body).map(|b| b.breakpoints)
        })
    }

    /// Lists threads (and refreshes [`Self::known_threads`]).
    pub fn threads(&self) -> impl Future<Output = Result<Vec<Thread>>> + Send + 'static {
        self.spawn(move |core| async move {
            let conn = core.connection(false).await?;
            let body = conn.request("threads", None, Some(REQUEST_TIMEOUT)).await?;
            let threads = parse_body::<ThreadsResponseBody>(body)?.threads;
            let generation = core.generation();
            let mut data = core.lock();
            if data.generation == generation {
                data.threads = threads.iter().map(|t| (t.id, t.clone())).collect();
            }
            Ok(threads)
        })
    }

    /// Stack frames of a stopped thread, innermost first.
    pub fn stack_trace(&self, thread_id: i64) -> impl Future<Output = Result<Vec<StackFrame>>> + Send + 'static {
        let args = StackTraceArguments {
            thread_id,
            start_frame: None,
            levels: None,
        };
        let fut = self.typed::<_, StackTraceResponseBody>("stackTrace", args);
        async move { Ok(fut.await?.stack_frames) }
    }

    /// Scopes of a frame.
    pub fn scopes(&self, frame_id: i64) -> impl Future<Output = Result<Vec<Scope>>> + Send + 'static {
        let fut = self.typed::<_, ScopesResponseBody>("scopes", ScopesArguments { frame_id });
        async move { Ok(fut.await?.scopes) }
    }

    /// Children of a scope or structured variable.
    pub fn variables(&self, variables_reference: i64) -> impl Future<Output = Result<Vec<Variable>>> + Send + 'static {
        let args = VariablesArguments { variables_reference };
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

    /// Resumes `thread_id` (usually all threads). The state becomes
    /// `Running` right away and is restored if the request fails.
    pub fn continue_(&self, thread_id: i64) -> impl Future<Output = Result<ContinueResponseBody>> + Send + 'static {
        self.resume("continue", serde_json::to_value(ContinueArguments { thread_id }))
    }

    /// Steps over.
    pub fn next(&self, thread_id: i64) -> impl Future<Output = Result<()>> + Send + 'static {
        self.step("next", thread_id)
    }

    /// Steps into.
    pub fn step_in(&self, thread_id: i64) -> impl Future<Output = Result<()>> + Send + 'static {
        self.step("stepIn", thread_id)
    }

    /// Steps out.
    pub fn step_out(&self, thread_id: i64) -> impl Future<Output = Result<()>> + Send + 'static {
        self.step("stepOut", thread_id)
    }

    fn step(&self, command: &'static str, thread_id: i64) -> impl Future<Output = Result<()>> + Send + 'static {
        let fut = self.resume::<Value>(command, serde_json::to_value(StepArguments { thread_id }));
        async move { fut.await.map(|_| ()) }
    }

    fn resume<T>(
        &self,
        command: &'static str,
        args: serde_json::Result<Value>,
    ) -> impl Future<Output = Result<T>> + Send + 'static
    where
        T: DeserializeOwned + Default + Send + 'static,
    {
        self.spawn(move |core| async move {
            let args = args?;
            let conn = core.connection(false).await?;
            let generation = core.generation();
            let previous = core.transition(generation, SessionState::is_stopped, SessionState::Running);
            let result = conn.request(command, Some(args), Some(REQUEST_TIMEOUT)).await;
            if result.is_err()
                && let Some(previous) = previous
            {
                core.transition(generation, |s| *s == SessionState::Running, previous);
            }
            parse_body(result?)
        })
    }

    /// Asks the adapter to pause `thread_id`; a `stopped` event follows.
    pub fn pause(&self, thread_id: i64) -> impl Future<Output = Result<()>> + Send + 'static {
        let fut = self.typed::<_, Value>("pause", PauseArguments { thread_id });
        async move { fut.await.map(|_| ()) }
    }

    /// Restarts: the adapter's `restart` if supported, otherwise disconnect
    /// and start a fresh adapter with the same config and current
    /// breakpoints. Resolves once the new run is configured.
    pub fn restart(&self) -> impl Future<Output = Result<()>> + Send + 'static {
        self.spawn(control::restart)
    }

    /// Stops: `terminate` if supported (else `disconnect`), then kills the
    /// adapter after [`STOP_TIMEOUT`]. The state ends as `Terminated`.
    pub fn stop(&self) -> impl Future<Output = Result<()>> + Send + 'static {
        self.spawn(control::stop)
    }

    /// Kills the adapter immediately.
    pub fn kill(&self) {
        let generation = self.core().generation();
        control::finish_stopped(self.core(), generation, "killed");
    }

    /// Waits until the state satisfies `pred` (or `timeout` passes);
    /// returns the state reached. Must be awaited inside tokio for the
    /// timeout; useful in tests and scripted runs.
    pub async fn wait_for_state(
        &self,
        timeout: Duration,
        pred: impl Fn(&SessionState) -> bool,
    ) -> Option<SessionState> {
        let mut rx = self.core().subscribe_state();
        let reached = tokio::time::timeout(timeout, rx.wait_for(|s| pred(s))).await;
        match reached {
            Ok(Ok(state)) => Some(state.clone()),
            _ => None,
        }
    }
}

fn parse_body<T: DeserializeOwned + Default>(body: Option<Value>) -> Result<T> {
    match body {
        None | Some(Value::Null) => Ok(T::default()),
        Some(v) => serde_json::from_value(v).map_err(DapError::from),
    }
}
