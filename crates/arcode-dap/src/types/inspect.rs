//! Threads, stack frames, scopes, variables, evaluation and execution
//! control.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Source;

/// A thread of the debuggee.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Thread {
    /// Thread id.
    pub id: i64,
    /// Display name.
    #[serde(default)]
    pub name: String,
}

/// Body of the `threads` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadsResponseBody {
    /// All threads.
    #[serde(default)]
    pub threads: Vec<Thread>,
}

/// Arguments of `stackTrace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceArguments {
    /// Thread to inspect.
    pub thread_id: i64,
    /// Index of the first frame to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<u32>,
    /// Maximum number of frames (absent or 0: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<u32>,
}

/// One stack frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    /// Frame id (for `scopes` and `evaluate`).
    pub id: i64,
    /// Display name (usually the function).
    #[serde(default)]
    pub name: String,
    /// Source location, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// 1-based line (0 if unknown).
    #[serde(default)]
    pub line: u32,
    /// 1-based column (0 if unknown).
    #[serde(default)]
    pub column: u32,
    /// `"normal"`, `"label"` or `"subtle"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<String>,
}

/// Body of the `stackTrace` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceResponseBody {
    /// Frames, innermost first.
    #[serde(default)]
    pub stack_frames: Vec<StackFrame>,
    /// Total number of frames available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_frames: Option<u32>,
}

/// Arguments of `scopes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesArguments {
    /// Frame to inspect.
    pub frame_id: i64,
}

/// A variable scope of a frame (locals, arguments, registers, ...).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    /// Display name.
    pub name: String,
    /// Pass to `variables` to list the scope's content.
    pub variables_reference: i64,
    /// Fetching the content is expensive (show collapsed).
    #[serde(default)]
    pub expensive: bool,
    /// `"arguments"`, `"locals"`, `"registers"`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<String>,
}

/// Body of the `scopes` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopesResponseBody {
    /// The frame's scopes.
    #[serde(default)]
    pub scopes: Vec<Scope>,
}

/// Arguments of `variables`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArguments {
    /// A scope's or structured variable's reference.
    pub variables_reference: i64,
}

/// A variable (or a child of a structured value).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// Name.
    pub name: String,
    /// Value rendered as text.
    #[serde(default)]
    pub value: String,
    /// Type, if the adapter reports it.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Non-zero if the value has children (pass to `variables`).
    #[serde(default)]
    pub variables_reference: i64,
    /// Expression that evaluates to this variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
}

/// Body of the `variables` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariablesResponseBody {
    /// The variables.
    #[serde(default)]
    pub variables: Vec<Variable>,
}

/// Arguments of `continue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueArguments {
    /// Thread to continue (most adapters continue all threads).
    pub thread_id: i64,
}

/// Body of the `continue` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueResponseBody {
    /// Absent or true: all threads were continued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

/// Arguments of `next`, `stepIn` and `stepOut`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepArguments {
    /// Thread to step.
    pub thread_id: i64,
}

/// Arguments of `pause`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseArguments {
    /// Thread to pause.
    pub thread_id: i64,
}

/// Where an expression is evaluated; adapters format results accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvaluateContext {
    /// Debug console input (may have side effects).
    Repl,
    /// Hovering over an identifier in the editor.
    Hover,
    /// A watch expression.
    Watch,
}

/// Arguments of `evaluate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateArguments {
    /// The expression.
    pub expression: String,
    /// Frame for name lookup (absent: global scope).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
    /// Evaluation context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<EvaluateContext>,
}

/// Body of the `evaluate` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResponseBody {
    /// The result as text.
    #[serde(default)]
    pub result: String,
    /// Type, if known.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Non-zero if the result has children.
    #[serde(default)]
    pub variables_reference: i64,
}

/// Arguments of `disconnect`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectArguments {
    /// The disconnect is part of a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
    /// Kill the debuggee (default: adapter decides, usually yes for launch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminate_debuggee: Option<bool>,
}

/// Arguments of `terminate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TerminateArguments {
    /// The termination is part of a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
}

/// Arguments of `restart`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RestartArguments {
    /// The latest launch/attach arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Breakpoint, SetBreakpointsResponseBody};
    use serde_json::json;

    #[test]
    fn lenient_parsing_of_adapter_data() {
        let frame: StackFrame = serde_json::from_value(json!({
            "id": 1000, "name": "main", "line": 3, "column": 0,
            "source": {"path": "/a/main.rs", "name": "main.rs", "adapterData": {}},
            "moduleId": 7
        }))
        .unwrap();
        assert_eq!(frame.source.unwrap().path.as_deref(), Some("/a/main.rs"));
        let var: Variable = serde_json::from_value(json!({
            "name": "v", "value": "[1, 2]", "type": "Vec<i32>", "variablesReference": 12,
            "presentationHint": {"kind": "data"}
        }))
        .unwrap();
        assert_eq!(var.type_.as_deref(), Some("Vec<i32>"));
        let bp: Breakpoint = serde_json::from_value(json!({"verified": false})).unwrap();
        assert_eq!(bp.id, None);
        let body: SetBreakpointsResponseBody = serde_json::from_value(json!({})).unwrap();
        assert!(body.breakpoints.is_empty());
    }

    #[test]
    fn evaluate_context_is_lowercase() {
        let args = EvaluateArguments {
            expression: "x".into(),
            frame_id: Some(1),
            context: Some(EvaluateContext::Hover),
        };
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            json!({"expression": "x", "frameId": 1, "context": "hover"})
        );
    }
}
