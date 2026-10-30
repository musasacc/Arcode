//! A scripted in-process debug adapter replaying a realistic session, and
//! helpers for waiting on events.
#![allow(dead_code)]

pub mod setup;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcode_dap::transport::{read_message, write_message};
use arcode_dap::{DebugEvent, EventReceiver};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// How the fake adapter behaves.
#[derive(Debug, Clone, Default)]
pub struct Behavior {
    pub supports_terminate: bool,
    pub supports_restart: bool,
    /// Send `initialized` after the `launch` request (codelldb style)
    /// instead of right after the `initialize` response.
    pub initialized_after_launch: bool,
    /// Answer `launch` only after `configurationDone` (debugpy style).
    pub launch_after_configuration_done: bool,
    /// Ask the client to `runInTerminal` during launch.
    pub run_in_terminal: bool,
    /// Also send a `startDebugging` reverse request during launch.
    pub start_debugging: bool,
    /// Hold the first `stackTrace`/`scopes` response of the session until
    /// the next request was answered (out-of-order responses).
    pub out_of_order: bool,
    /// Drop the connection on receiving this command.
    pub crash_on: Option<&'static str>,
    /// Fail `launch` with this message.
    pub fail_launch: Option<&'static str>,
}

/// Every message the fake received, in order.
#[derive(Debug, Clone, Default)]
pub struct Received(pub Arc<Mutex<Vec<Value>>>);

impl Received {
    pub fn all(&self) -> Vec<Value> {
        self.0.lock().unwrap().clone()
    }

    /// Commands of received requests, in order.
    pub fn commands(&self) -> Vec<String> {
        self.all()
            .iter()
            .filter(|m| m["type"] == "request")
            .filter_map(|m| m["command"].as_str().map(String::from))
            .collect()
    }

    /// Received responses (to reverse requests).
    pub fn responses(&self) -> Vec<Value> {
        self.all()
            .into_iter()
            .filter(|m| m["type"] == "response")
            .collect()
    }
}

struct Out<W> {
    writer: W,
    seq: i64,
}

impl<W: AsyncWrite + Unpin> Out<W> {
    async fn send(&mut self, mut msg: Value) {
        self.seq += 1;
        msg["seq"] = json!(self.seq);
        let _ = write_message(&mut self.writer, &msg.to_string()).await;
    }

    async fn event(&mut self, event: &str, body: Value) {
        self.send(json!({"type": "event", "event": event, "body": body}))
            .await;
    }
}

fn response(req: &Value, body: Value) -> Value {
    json!({"type": "response", "request_seq": req["seq"], "success": true,
        "command": req["command"], "body": body})
}

fn error(req: &Value, format: &str, vars: Value) -> Value {
    json!({"type": "response", "request_seq": req["seq"], "success": false,
        "command": req["command"], "message": "failed",
        "body": {"error": {"id": 1, "format": format, "variables": vars}}})
}

/// Runs the fake adapter until the client disconnects or the stream ends.
pub async fn run_fake<R, W>(reader: R, writer: W, behavior: Behavior, received: Received)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut out = Out { writer, seq: 0 };
    let mut pending_launch: Option<Value> = None;
    let mut stashed: Option<Value> = None;
    let mut stash_used = false;
    let mut next_bp_id = 1;
    while let Ok(Some(text)) = read_message(&mut reader).await {
        let Ok(req) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        received.0.lock().unwrap().push(req.clone());
        if req["type"] != "request" {
            continue;
        }
        let command = req["command"].as_str().unwrap_or_default().to_string();
        if behavior.crash_on == Some(command.as_str()) {
            return;
        }
        let mut reply = None;
        match command.as_str() {
            "initialize" => {
                let mut caps = json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsFunctionBreakpoints": true,
                    "supportsConditionalBreakpoints": true,
                    "supportsEvaluateForHovers": true,
                    "exceptionBreakpointFilters": [
                        {"filter": "panic", "label": "Panics", "default": true}
                    ],
                    "supportsTerminateRequest": behavior.supports_terminate,
                    "supportsRestartRequest": behavior.supports_restart,
                });
                caps["somethingUnknown"] = json!({"nested": true});
                out.send(response(&req, caps)).await;
                if !behavior.initialized_after_launch {
                    out.event("initialized", Value::Null).await;
                }
            }
            "launch" | "attach" => {
                if let Some(message) = behavior.fail_launch {
                    out.send(error(&req, "cannot launch: {why}", json!({"why": message})))
                        .await;
                    continue;
                }
                if behavior.run_in_terminal {
                    out.send(json!({"type": "request", "command": "runInTerminal",
                        "arguments": {"kind": "integrated", "title": "app", "cwd": "/work",
                            "args": ["/work/app", "--x"], "env": {"A": "1", "B": null}}}))
                        .await;
                }
                if behavior.start_debugging {
                    out.send(json!({"type": "request", "command": "startDebugging",
                        "arguments": {"request": "launch", "configuration": {}}}))
                        .await;
                }
                if behavior.launch_after_configuration_done {
                    pending_launch = Some(req.clone());
                } else {
                    out.send(response(&req, Value::Null)).await;
                }
                if behavior.initialized_after_launch {
                    out.event("initialized", Value::Null).await;
                }
            }
            "setBreakpoints" => {
                let path = req["arguments"]["source"]["path"].clone();
                let bps: Vec<Value> = req["arguments"]["breakpoints"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|bp| {
                        next_bp_id += 1;
                        json!({"id": next_bp_id - 1, "verified": bp["line"] != 999,
                            "line": bp["line"], "source": {"path": path}})
                    })
                    .collect();
                reply = Some(response(&req, json!({"breakpoints": bps})));
            }
            "setFunctionBreakpoints" => {
                let n = req["arguments"]["breakpoints"]
                    .as_array()
                    .map_or(0, Vec::len);
                let bps: Vec<Value> = (0..n).map(|_| json!({"verified": true})).collect();
                reply = Some(response(&req, json!({"breakpoints": bps})));
            }
            "setExceptionBreakpoints" => reply = Some(response(&req, Value::Null)),
            "configurationDone" => {
                out.send(response(&req, Value::Null)).await;
                if let Some(launch) = pending_launch.take() {
                    out.send(response(&launch, Value::Null)).await;
                }
                out.event(
                    "process",
                    json!({"name": "/work/app", "systemProcessId": 4242,
                    "startMethod": "launch"}),
                )
                .await;
                out.event("thread", json!({"reason": "started", "threadId": 1}))
                    .await;
                out.event(
                    "module",
                    json!({"reason": "new", "module": {"id": 1, "name": "app"}}),
                )
                .await;
                out.event("output", json!({"category": "stdout", "output": "hello\n"}))
                    .await;
                out.event(
                    "stopped",
                    json!({"reason": "breakpoint", "threadId": 1,
                    "allThreadsStopped": true, "hitBreakpointIds": [1]}),
                )
                .await;
            }
            "threads" => {
                reply = Some(response(
                    &req,
                    json!({"threads": [
                    {"id": 1, "name": "main"}, {"id": 2, "name": "worker"}]}),
                ));
            }
            "stackTrace" => {
                reply = Some(response(
                    &req,
                    json!({"stackFrames": [
                    {"id": 1000, "name": "app::main", "line": 3, "column": 5,
                        "source": {"name": "main.rs", "path": "/work/src/main.rs"}},
                    {"id": 1001, "name": "std::rt::lang_start", "line": 0, "column": 0,
                        "presentationHint": "subtle"}
                ], "totalFrames": 2}),
                ));
            }
            "scopes" => {
                reply = Some(response(
                    &req,
                    json!({"scopes": [
                    {"name": "Locals", "variablesReference": 100, "expensive": false},
                    {"name": "Registers", "variablesReference": 200, "expensive": true}]}),
                ));
            }
            "variables" => {
                let vars = match req["arguments"]["variablesReference"].as_i64() {
                    Some(100) => json!([
                        {"name": "x", "value": "42", "type": "i32", "variablesReference": 0},
                        {"name": "v", "value": "size=2", "type": "Vec<i32>",
                            "variablesReference": 101}]),
                    Some(101) => json!([
                        {"name": "[0]", "value": "1", "type": "i32", "variablesReference": 0},
                        {"name": "[1]", "value": "2", "type": "i32", "variablesReference": 0}]),
                    _ => json!([]),
                };
                reply = Some(response(&req, json!({"variables": vars})));
            }
            "evaluate" => {
                let expr = req["arguments"]["expression"].as_str().unwrap_or_default();
                reply = Some(if expr == "nope" {
                    error(&req, "unknown name `{name}`", json!({"name": expr}))
                } else {
                    response(
                        &req,
                        json!({"result": format!("{expr} = 42"), "type": "i32",
                        "variablesReference": 0}),
                    )
                });
            }
            "continue" => {
                out.send(response(&req, json!({"allThreadsContinued": true})))
                    .await;
                out.event("output", json!({"category": "stdout", "output": "bye\n"}))
                    .await;
                out.event("exited", json!({"exitCode": 0})).await;
                out.event("terminated", json!({})).await;
            }
            "next" | "stepIn" | "stepOut" => {
                out.send(response(&req, Value::Null)).await;
                let thread = req["arguments"]["threadId"].clone();
                out.event("stopped", json!({"reason": "step", "threadId": thread}))
                    .await;
            }
            "pause" => {
                out.send(response(&req, Value::Null)).await;
                out.event("stopped", json!({"reason": "pause", "threadId": 1}))
                    .await;
            }
            "restart" => {
                out.send(response(&req, Value::Null)).await;
                out.event("initialized", Value::Null).await;
            }
            "terminate" => {
                out.send(response(&req, Value::Null)).await;
                out.event("terminated", json!({})).await;
            }
            "disconnect" => {
                out.send(response(&req, Value::Null)).await;
                return;
            }
            _ => reply = Some(error(&req, "unsupported", json!({}))),
        }
        let Some(reply) = reply else { continue };
        let delayable = matches!(command.as_str(), "stackTrace" | "scopes");
        if behavior.out_of_order && delayable && !stash_used {
            stash_used = true;
            stashed = Some(reply);
            continue;
        }
        out.send(reply).await;
        if let Some(late) = stashed.take() {
            out.send(late).await;
        }
    }
}

/// Waits (up to 10 s) for the first event matching `pred`, skipping others.
pub async fn wait_event(rx: &EventReceiver, pred: impl Fn(&DebugEvent) -> bool) -> DebugEvent {
    let fut = async {
        loop {
            match rx.recv().await {
                Ok(e) if pred(&e) => return e,
                Ok(_) => {}
                Err(_) => panic!("event channel closed while waiting"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .expect("timed out waiting for an event")
}

/// Collects events until one matches `pred` (inclusive).
pub async fn collect_until(
    rx: &EventReceiver,
    pred: impl Fn(&DebugEvent) -> bool,
) -> Vec<DebugEvent> {
    let fut = async {
        let mut out = Vec::new();
        loop {
            match rx.recv().await {
                Ok(e) => {
                    let done = pred(&e);
                    out.push(e);
                    if done {
                        return out;
                    }
                }
                Err(_) => panic!("event channel closed; got {out:?}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .expect("timed out collecting events")
}
