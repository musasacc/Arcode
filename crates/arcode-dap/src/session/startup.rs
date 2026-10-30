//! The start sequence: connect, `initialize`, `launch`/`attach`, wait for
//! `initialized`, configure, `configurationDone`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{oneshot, watch};

use super::core::{Core, Phase};
use super::inbound;
use crate::connection::Connection;
use crate::error::{DapError, Result};
use crate::event::{DebugEvent, SessionState};
use crate::process::{self, BoxReader, BoxWriter};
use crate::types::{
    Breakpoint, Capabilities, InitializeRequestArguments, SetBreakpointsArguments,
    SetBreakpointsResponseBody, SetExceptionBreakpointsArguments, SetFunctionBreakpointsArguments,
    Source, SourceBreakpoint,
};

/// Limit for `initialize`.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// Limit for ordinary requests.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// If `launch`/`attach` succeeded but no `initialized` event came within
/// this time, configuration is sent anyway.
const INITIALIZED_GRACE: Duration = Duration::from_secs(5);

/// Runs the start sequence for `generation`; failures end the generation
/// with [`DebugEvent::AdapterFailed`].
pub(crate) async fn run(core: Arc<Core>, generation: u64, streams: Option<(BoxReader, BoxWriter)>) {
    if let Err(e) = start(&core, generation, streams).await {
        core.fail(generation, e.to_string()).await;
    }
}

async fn start(
    core: &Arc<Core>,
    generation: u64,
    streams: Option<(BoxReader, BoxWriter)>,
) -> Result<()> {
    let conn = connect(core, generation, streams).await?;
    let (adapter_id, request, arguments, configure_first) = {
        let data = core.lock();
        let c = &data.config;
        let a = &c.adapter;
        (
            a.adapter_id.clone(),
            c.request,
            c.arguments.clone(),
            a.configure_before_launch,
        )
    };

    // Registered before `initialize`: adapters may send `initialized`
    // right behind the response.
    let (init_tx, mut init_rx) = oneshot::channel();
    core.lock().initialized_tx = Some(init_tx);
    let init = serde_json::to_value(InitializeRequestArguments::new(adapter_id))?;
    let body = conn
        .request("initialize", Some(init), Some(INITIALIZE_TIMEOUT))
        .await?;
    let caps: Capabilities = match body {
        Some(v) if !v.is_null() => serde_json::from_value(v)?,
        _ => Capabilities::default(),
    };
    {
        let mut data = core.lock();
        if data.generation != generation {
            return Ok(());
        }
        data.capabilities = Some(caps.clone());
    }
    core.events.send(DebugEvent::Capabilities(caps));
    core.set_phase(generation, Phase::Ready(conn.clone()));

    if configure_first {
        // Quirk: the adapter acts on `launch` at once (gdb ≤ 15 runs the
        // program), so configure first and launch last.
        wait_initialized(core, &mut init_rx).await?;
        configure(core, generation, &conn).await?;
        core.set_phase(generation, Phase::Configured(conn.clone()));
        conn.request(request.command(), Some(arguments), None)
            .await?;
    } else {
        // `launch` is answered by some adapters (debugpy) only after
        // `configurationDone`, so it is not awaited before configuring.
        // It also resolves (with an error) if the adapter dies.
        let launch = conn.request(request.command(), Some(arguments), None);
        tokio::pin!(launch);
        let mut launched = false;
        tokio::select! {
            r = &mut init_rx => {
                r.map_err(|_| superseded())?;
                core.events.send(DebugEvent::Initialized);
            }
            r = &mut launch => {
                r?;
                launched = true;
                wait_initialized(core, &mut init_rx).await?;
            }
        }
        configure(core, generation, &conn).await?;
        core.set_phase(generation, Phase::Configured(conn.clone()));
        if !launched {
            launch.await?;
        }
    }
    core.transition(
        generation,
        |s| *s == SessionState::Initializing,
        SessionState::Running,
    );
    Ok(())
}

fn superseded() -> DapError {
    DapError::SessionEnded("superseded by restart".into())
}

/// Waits up to [`INITIALIZED_GRACE`] for the `initialized` event; after
/// that configuration is sent anyway.
async fn wait_initialized(core: &Core, init_rx: &mut oneshot::Receiver<()>) -> Result<()> {
    match tokio::time::timeout(INITIALIZED_GRACE, init_rx).await {
        Ok(Ok(())) => core.events.send(DebugEvent::Initialized),
        Ok(Err(_)) => return Err(superseded()),
        Err(_) => log::warn!("adapter sent no `initialized` event; configuring anyway"),
    }
    Ok(())
}

/// Opens the transport, starts the inbound processor and log forwarders.
async fn connect(
    core: &Arc<Core>,
    generation: u64,
    streams: Option<(BoxReader, BoxWriter)>,
) -> Result<Arc<Connection>> {
    let (reader, writer, child, logs) = match streams {
        Some((r, w)) => (r, w, None, Vec::new()),
        None => {
            let spec = core.lock().config.adapter.clone();
            let opened = process::open(&spec).await?;
            (opened.reader, opened.writer, opened.child, opened.logs)
        }
    };
    let (kill_tx, kill_rx) = oneshot::channel();
    let kill = child.as_ref().map(|_| kill_tx);
    let (conn, inbound_rx) = Connection::spawn(reader, writer, kill);
    if let Some(child) = child {
        tokio::spawn(process::supervise(conn.clone(), child, kill_rx));
    }
    let (done_tx, done_rx) = watch::channel(logs.is_empty());
    let done_tx = Arc::new(done_tx);
    let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(logs.len()));
    for log in logs {
        let (core, done_tx, remaining) = (core.clone(), done_tx.clone(), remaining.clone());
        tokio::spawn(async move {
            process::forward_lines(log, |line| core.log_line(generation, line)).await;
            if remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                done_tx.send_replace(true);
            }
        });
    }
    {
        let mut data = core.lock();
        if data.generation != generation || data.stopping {
            conn.kill("session stopped while starting");
            return Err(DapError::SessionEnded(
                "session stopped while starting".into(),
            ));
        }
        data.conn = Some(conn.clone());
        data.logs_done = Some(done_rx);
    }
    tokio::spawn(inbound::process(
        core.clone(),
        generation,
        conn.clone(),
        inbound_rx,
    ));
    Ok(conn)
}

/// Sends breakpoints, exception filters and `configurationDone`.
pub(crate) async fn configure(
    core: &Arc<Core>,
    generation: u64,
    conn: &Arc<Connection>,
) -> Result<()> {
    let (breakpoints, functions, filters, caps) = {
        let data = core.lock();
        let c = &data.config;
        (
            c.breakpoints.clone(),
            c.function_breakpoints.clone(),
            c.exception_filters.clone(),
            data.capabilities.clone().unwrap_or_default(),
        )
    };
    for (path, bps) in breakpoints {
        if let Err(e) = set_breakpoints(core, generation, conn, path.clone(), bps).await {
            log::warn!("setBreakpoints for {} failed: {e}", path.display());
        }
    }
    if caps.supports_function_breakpoints && !functions.is_empty() {
        let args = serde_json::to_value(SetFunctionBreakpointsArguments {
            breakpoints: functions,
        })?;
        if let Err(e) = conn
            .request("setFunctionBreakpoints", Some(args), Some(REQUEST_TIMEOUT))
            .await
        {
            log::warn!("setFunctionBreakpoints failed: {e}");
        }
    }
    if caps.exception_breakpoint_filters.is_some() {
        let args = serde_json::to_value(SetExceptionBreakpointsArguments { filters })?;
        if let Err(e) = conn
            .request("setExceptionBreakpoints", Some(args), Some(REQUEST_TIMEOUT))
            .await
        {
            log::warn!("setExceptionBreakpoints failed: {e}");
        }
    }
    if caps.supports_configuration_done_request {
        conn.request("configurationDone", Some(json!({})), Some(REQUEST_TIMEOUT))
            .await?;
    }
    Ok(())
}

/// Sends `setBreakpoints` for one file and reports the result as
/// [`DebugEvent::BreakpointsSet`].
pub(crate) async fn set_breakpoints(
    core: &Core,
    generation: u64,
    conn: &Arc<Connection>,
    path: std::path::PathBuf,
    breakpoints: Vec<SourceBreakpoint>,
) -> Result<Vec<Breakpoint>> {
    let args = SetBreakpointsArguments {
        source: Source {
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path: Some(path.to_string_lossy().into_owned()),
            source_reference: None,
        },
        breakpoints,
    };
    let body = conn
        .request(
            "setBreakpoints",
            Some(serde_json::to_value(args)?),
            Some(REQUEST_TIMEOUT),
        )
        .await?;
    let body: SetBreakpointsResponseBody = match body {
        Some(v) if !v.is_null() => serde_json::from_value(v)?,
        _ => SetBreakpointsResponseBody::default(),
    };
    if core.is_current(generation) {
        core.events.send(DebugEvent::BreakpointsSet {
            path,
            breakpoints: body.breakpoints.clone(),
        });
    }
    Ok(body.breakpoints)
}
