//! Stopping and restarting a session.

use std::sync::Arc;
use std::time::Duration;

use super::core::Core;
use super::startup;
use crate::connection::Connection;
use crate::error::{DapError, Result};
use crate::event::{DebugEvent, SessionState};
use crate::types::{DisconnectArguments, RestartArguments, TerminateArguments};

/// Limit for each phase of a stop (terminate, disconnect, adapter exit).
pub const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Ends a connection: optional `terminate` (if supported) and waiting for
/// `terminated`, then `disconnect`, then waiting for the adapter to close
/// the connection; kills the adapter if any step times out.
pub(crate) async fn shutdown(core: &Core, conn: Arc<Connection>, terminate: bool, restart: bool) {
    let caps = core.capabilities();
    if terminate && caps.supports_terminate_request && conn.is_alive() {
        let mut state = core.subscribe_state();
        let args = serde_json::to_value(TerminateArguments {
            restart: restart.then_some(true),
        })
        .ok();
        if conn
            .request("terminate", args, Some(STOP_TIMEOUT))
            .await
            .is_ok()
        {
            let terminated = state.wait_for(|s| *s == SessionState::Terminated);
            tokio::select! {
                _ = tokio::time::timeout(STOP_TIMEOUT, terminated) => {}
                _ = conn.closed() => {}
            }
        }
    }
    if conn.is_alive() {
        let args = serde_json::to_value(DisconnectArguments {
            restart: restart.then_some(true),
            terminate_debuggee: Some(true),
        })
        .ok();
        if let Err(e) = conn.request("disconnect", args, Some(STOP_TIMEOUT)).await {
            log::debug!("disconnect failed: {e}");
        }
    }
    if tokio::time::timeout(STOP_TIMEOUT, conn.closed())
        .await
        .is_err()
    {
        conn.kill("adapter did not exit after disconnect");
    } else {
        // The connection is closed; make sure the process is gone too.
        conn.kill("stopped");
    }
}

/// Stops the debuggee and the adapter.
pub(crate) async fn stop(core: Arc<Core>) -> Result<()> {
    let (generation, conn) = {
        let mut data = core.lock();
        data.stopping = true;
        (data.generation, data.conn.clone())
    };
    if let Some(conn) = conn {
        shutdown(&core, conn, true, false).await;
    }
    finish_stopped(&core, generation, "stopped");
    Ok(())
}

/// Ends `generation` after a deliberate stop and kills its adapter
/// (reports [`DebugEvent::AdapterExited`] unless already ended).
pub(crate) fn finish_stopped(core: &Core, generation: u64, reason: &str) {
    let conn = {
        let mut data = core.lock();
        if data.generation != generation {
            return;
        }
        data.stopping = true;
        data.conn.clone()
    };
    core.end_generation(generation, reason.to_string(), |_| {
        DebugEvent::AdapterExited(reason.to_string())
    });
    if let Some(conn) = conn {
        conn.kill(reason);
    }
}

/// The last handle was dropped: forbids new generations, then ends the
/// current one and kills its adapter.
pub(crate) fn finish_dropped(core: &Core) {
    let generation = {
        let mut data = core.lock();
        data.dropped = true;
        data.generation
    };
    // The generation cannot change any more (`next_generation` refuses).
    finish_stopped(core, generation, "debug session dropped");
}

/// Restarts with the adapter's `restart` request if supported, otherwise
/// by disconnecting and starting a fresh adapter with the same config
/// (and the current breakpoints).
pub(crate) async fn restart(core: Arc<Core>) -> Result<()> {
    let conn = core.current_connection().filter(|c| c.is_alive());
    let configured = match &conn {
        Some(_) => core.connection(true).await.ok(),
        None => None,
    };
    if let Some(conn) = configured
        && core.capabilities().supports_restart_request
    {
        let (generation, arguments) = {
            let data = core.lock();
            (data.generation, data.config.arguments.clone())
        };
        let previous = core.transition(
            generation,
            |s| *s != SessionState::Terminated,
            SessionState::Running,
        );
        let args = serde_json::to_value(RestartArguments {
            arguments: Some(arguments),
        })?;
        let result = conn
            .request("restart", Some(args), Some(startup::REQUEST_TIMEOUT))
            .await;
        if result.is_err()
            && let Some(previous) = previous
        {
            core.transition(generation, |s| *s == SessionState::Running, previous);
        }
        return result.map(|_| ());
    }
    if !core.lock().respawnable {
        return Err(DapError::Unsupported(
            "restart: the adapter has no `restart` request and the session cannot respawn it"
                .into(),
        ));
    }
    // Refused once the last handle is gone: nobody could stop the new one.
    let Some(generation) = core.next_generation() else {
        return Err(DapError::SessionEnded("debug session dropped".into()));
    };
    core.set_state(generation, SessionState::Initializing);
    if let Some(old) = conn {
        shutdown(&core, old, false, true).await;
    }
    tokio::spawn(startup::run(core.clone(), generation, None));
    core.connection(true).await.map(|_| ())
}
