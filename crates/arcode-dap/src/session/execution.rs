//! Execution control: continue, stepping, pause, restart, stop.

use std::future::Future;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{DebugSessionHandle, REQUEST_TIMEOUT, control, parse_body};
use crate::error::Result;
use crate::event::SessionState;
use crate::types::{ContinueArguments, ContinueResponseBody, PauseArguments, StepArguments};

impl DebugSessionHandle {
    /// Resumes `thread_id` (usually all threads). The state becomes
    /// `Running` right away and is restored if the request fails.
    pub fn continue_(
        &self,
        thread_id: i64,
    ) -> impl Future<Output = Result<ContinueResponseBody>> + Send + 'static {
        self.resume(
            "continue",
            serde_json::to_value(ContinueArguments { thread_id }),
        )
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

    fn step(
        &self,
        command: &'static str,
        thread_id: i64,
    ) -> impl Future<Output = Result<()>> + Send + 'static {
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
            let previous =
                core.transition(generation, SessionState::is_stopped, SessionState::Running);
            let result = conn
                .request(command, Some(args), Some(REQUEST_TIMEOUT))
                .await;
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
    /// adapter after [`crate::STOP_TIMEOUT`]. The state ends as `Terminated`.
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
