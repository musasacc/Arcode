//! Hand-written serde types for the subset of the Debug Adapter Protocol
//! Arcode uses. Unknown fields are ignored; optional fields are skipped
//! when serializing.
//!
//! Numbers that identify things (threads, frames, variable references) are
//! `i64`; line and column numbers are 1-based `u32` (the client announces
//! `linesStartAt1` / `columnsStartAt1`).

mod capabilities;
mod events;
mod inspect;
mod requests;

pub use capabilities::{Capabilities, ExceptionBreakpointsFilter};
pub use events::{
    BreakpointEventBody, ContinuedEventBody, ExitedEventBody, OutputEventBody, ProcessEventBody,
    RunInTerminalRequestArguments, RunInTerminalResponseBody, StartDebuggingRequestArguments,
    StoppedEventBody, TerminatedEventBody, ThreadEventBody,
};
pub use inspect::{
    ContinueArguments, ContinueResponseBody, DisconnectArguments, EvaluateArguments,
    EvaluateContext, EvaluateResponseBody, PauseArguments, RestartArguments, Scope,
    ScopesArguments, ScopesResponseBody, StackFrame, StackTraceArguments, StackTraceResponseBody,
    StepArguments, TerminateArguments, Thread, ThreadsResponseBody, Variable, VariablesArguments,
    VariablesResponseBody,
};
pub use requests::{
    Breakpoint, FunctionBreakpoint, InitializeRequestArguments, SetBreakpointsArguments,
    SetBreakpointsResponseBody, SetExceptionBreakpointsArguments, SetFunctionBreakpointsArguments,
    Source, SourceBreakpoint,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One message on the wire: `{"seq": n, "type": "request" | "response" | "event", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProtocolMessage {
    /// A request (client to adapter, or a reverse request from the adapter).
    Request(Request),
    /// A response to a request.
    Response(Response),
    /// An event from the adapter.
    Event(Event),
}

impl ProtocolMessage {
    /// The message's sequence number.
    pub fn seq(&self) -> i64 {
        match self {
            ProtocolMessage::Request(r) => r.seq,
            ProtocolMessage::Response(r) => r.seq,
            ProtocolMessage::Event(e) => e.seq,
        }
    }
}

/// A request message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Sequence number of this message.
    pub seq: i64,
    /// Command name, e.g. `"initialize"`.
    pub command: String,
    /// Command arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// A response message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Sequence number of this message.
    pub seq: i64,
    /// `seq` of the request this answers.
    pub request_seq: i64,
    /// Whether the request succeeded.
    pub success: bool,
    /// Command of the request this answers.
    pub command: String,
    /// Short error reason (e.g. `"cancelled"`) when `success` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Result body, or `{"error": Message}` on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl Response {
    /// Human-readable error text of a failed response: the formatted
    /// `body.error` message if present, else `message`.
    pub fn error_text(&self) -> String {
        let formatted = self
            .body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(format_error_message);
        formatted
            .or_else(|| self.message.clone())
            .unwrap_or_else(|| format!("{} failed", self.command))
    }
}

/// Formats a DAP `Message` (`format` with `{name}` placeholders filled from
/// `variables`).
fn format_error_message(error: &Value) -> Option<String> {
    let mut text = error.get("format")?.as_str()?.to_string();
    if let Some(vars) = error.get("variables").and_then(Value::as_object) {
        for (name, value) in vars {
            if let Some(value) = value.as_str() {
                text = text.replace(&format!("{{{name}}}"), value);
            }
        }
    }
    Some(text)
}

/// An event message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Sequence number of this message.
    pub seq: i64,
    /// Event name, e.g. `"stopped"`.
    pub event: String,
    /// Event-specific body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_all_three_kinds() {
        let req: ProtocolMessage = serde_json::from_value(json!({
            "seq": 3, "type": "request", "command": "runInTerminal",
            "arguments": {"cwd": "/", "args": ["a"]}
        }))
        .unwrap();
        assert!(matches!(&req, ProtocolMessage::Request(r) if r.command == "runInTerminal"));
        assert_eq!(req.seq(), 3);

        let resp: ProtocolMessage = serde_json::from_value(json!({
            "seq": 4, "type": "response", "request_seq": 1, "success": true,
            "command": "initialize", "body": {"supportsConfigurationDoneRequest": true},
            "unknownField": 1
        }))
        .unwrap();
        assert!(matches!(&resp, ProtocolMessage::Response(r) if r.request_seq == 1 && r.success));

        let ev: ProtocolMessage =
            serde_json::from_value(json!({"seq": 5, "type": "event", "event": "initialized"}))
                .unwrap();
        assert!(matches!(&ev, ProtocolMessage::Event(e) if e.body.is_none()));
    }

    #[test]
    fn serializes_request_without_null_arguments() {
        let msg = ProtocolMessage::Request(Request {
            seq: 1,
            command: "threads".into(),
            arguments: None,
        });
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"seq": 1, "type": "request", "command": "threads"})
        );
    }

    #[test]
    fn error_text_prefers_formatted_message() {
        let resp = Response {
            seq: 1,
            request_seq: 1,
            success: false,
            command: "evaluate".into(),
            message: Some("failed".into()),
            body: Some(json!({"error": {"id": 1, "format": "no symbol {name}",
                "variables": {"name": "x"}}})),
        };
        assert_eq!(resp.error_text(), "no symbol x");
        let bare = Response {
            body: None,
            message: None,
            ..resp
        };
        assert_eq!(bare.error_text(), "evaluate failed");
    }

    #[test]
    fn rejects_unknown_type() {
        let bad = serde_json::from_value::<ProtocolMessage>(json!({"seq": 1, "type": "weird"}));
        assert!(bad.is_err());
    }
}
