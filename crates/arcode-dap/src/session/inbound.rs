//! Processing adapter events and reverse requests in wire order.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use super::control;
use super::core::Core;
use super::startup;
use crate::connection::{Connection, Inbound};
use crate::event::{DebugEvent, RunInTerminalRequest, SessionState};
use crate::types::{
    BreakpointEventBody, ContinuedEventBody, Event, ExitedEventBody, OutputEventBody,
    ProcessEventBody, Request, RunInTerminalRequestArguments, StoppedEventBody,
    TerminatedEventBody, Thread, ThreadEventBody,
};

/// Consumes one connection's inbound queue until it closes.
pub(crate) async fn process(
    core: Arc<Core>,
    generation: u64,
    conn: Arc<Connection>,
    mut rx: mpsc::UnboundedReceiver<Inbound>,
) {
    while let Some(item) = rx.recv().await {
        if !core.is_current(generation) {
            if let Inbound::Request(r) = &item {
                let _ = conn.respond(r.seq, &r.command, Err("session restarted".into()));
            }
            continue;
        }
        match item {
            Inbound::Event(e) => on_event(&core, generation, &conn, e),
            Inbound::Request(r) => on_request(&core, &conn, r),
            Inbound::Closed(reason) => {
                core.on_closed(generation, reason).await;
                break;
            }
        }
    }
}

fn parse<T: DeserializeOwned + Default>(e: &Event) -> Option<T> {
    match &e.body {
        None | Some(serde_json::Value::Null) => Some(T::default()),
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(body) => Some(body),
            Err(err) => {
                log::warn!("ignoring malformed `{}` event: {err}", e.event);
                None
            }
        },
    }
}

fn on_event(core: &Arc<Core>, generation: u64, conn: &Arc<Connection>, e: Event) {
    let send = |event| core.events.send(event);
    let not_terminated = |s: &SessionState| *s != SessionState::Terminated;
    match e.event.as_str() {
        "initialized" => {
            send(DebugEvent::Initialized);
            let waiting = core.lock().initialized_tx.take();
            match waiting {
                Some(tx) => {
                    let _ = tx.send(());
                }
                // A later `initialized` (e.g. after `restart`): configure again.
                None => {
                    let (core, conn) = (core.clone(), conn.clone());
                    tokio::spawn(async move {
                        if let Err(e) = startup::configure(&core, generation, &conn).await {
                            log::warn!("reconfiguration failed: {e}");
                        }
                    });
                }
            }
        }
        "stopped" => {
            let Some(body) = parse::<StoppedEventBody>(&e) else { return };
            let state = SessionState::Stopped {
                thread_id: body.thread_id,
                reason: body.reason.clone(),
            };
            send(DebugEvent::Stopped(body));
            core.transition(generation, not_terminated, state);
        }
        "continued" => {
            let Some(body) = parse::<ContinuedEventBody>(&e) else { return };
            send(DebugEvent::Continued(body));
            core.transition(generation, SessionState::is_stopped, SessionState::Running);
        }
        "exited" => {
            let Some(body) = parse::<ExitedEventBody>(&e) else { return };
            core.lock().exit_code = Some(body.exit_code);
            send(DebugEvent::Exited(body));
        }
        "terminated" => {
            let body = parse::<TerminatedEventBody>(&e).unwrap_or_default();
            send(DebugEvent::Terminated(body));
            core.set_state(generation, SessionState::Terminated);
            if !core.lock().stopping {
                let (core, conn) = (core.clone(), conn.clone());
                tokio::spawn(async move { control::shutdown(&core, conn, false, false).await });
            }
        }
        "output" => {
            if let Some(body) = parse::<OutputEventBody>(&e) {
                send(DebugEvent::Output(body));
            }
        }
        "breakpoint" => {
            if let Some(body) = parse::<BreakpointEventBody>(&e) {
                send(DebugEvent::Breakpoint(body));
            }
        }
        "thread" => {
            let Some(body) = parse::<ThreadEventBody>(&e) else { return };
            {
                let mut data = core.lock();
                if body.reason == "exited" {
                    data.threads.remove(&body.thread_id);
                } else {
                    data.threads.entry(body.thread_id).or_insert_with(|| Thread {
                        id: body.thread_id,
                        name: format!("Thread {}", body.thread_id),
                    });
                }
            }
            send(DebugEvent::Thread(body));
        }
        "process" => {
            if let Some(body) = parse::<ProcessEventBody>(&e) {
                send(DebugEvent::Process(body));
            }
        }
        "capabilities" => {
            let update = e.body.as_ref().and_then(|b| b.get("capabilities"));
            if let Some(update) = update {
                let merged = {
                    let mut data = core.lock();
                    let caps = data.capabilities.get_or_insert_with(Default::default);
                    caps.merge_json(update);
                    caps.clone()
                };
                send(DebugEvent::Capabilities(merged));
            }
        }
        other => log::debug!("ignoring `{other}` event"),
    }
}

fn on_request(core: &Core, conn: &Arc<Connection>, r: Request) {
    match r.command.as_str() {
        "runInTerminal" => {
            let args = r.arguments.clone().unwrap_or_default();
            match serde_json::from_value::<RunInTerminalRequestArguments>(args) {
                Ok(arguments) => core
                    .events
                    .send(DebugEvent::RunInTerminal(RunInTerminalRequest::new(
                        arguments, conn, r.seq,
                    ))),
                Err(e) => {
                    let _ = conn.respond(r.seq, &r.command, Err(format!("bad arguments: {e}")));
                }
            }
        }
        "startDebugging" => {
            let _ = conn.respond(
                r.seq,
                &r.command,
                Err("startDebugging (child sessions) is not supported by Arcode yet".into()),
            );
        }
        other => {
            let _ = conn.respond(r.seq, other, Err(format!("unsupported request `{other}`")));
        }
    }
}
