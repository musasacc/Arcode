//! Request arguments and response bodies.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Arguments of the `initialize` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequestArguments {
    /// Always `"arcode"`.
    #[serde(rename = "clientID")]
    pub client_id: String,
    /// Human-readable client name.
    pub client_name: String,
    /// Adapter type, e.g. `"lldb"` or `"python"`.
    #[serde(rename = "adapterID")]
    pub adapter_id: String,
    /// Locale for messages, e.g. `"en-US"`.
    pub locale: String,
    /// Line numbers are 1-based.
    pub lines_start_at1: bool,
    /// Column numbers are 1-based.
    pub columns_start_at1: bool,
    /// Paths are native file system paths (not URIs).
    pub path_format: String,
    /// The client shows `Variable::type_`.
    pub supports_variable_type: bool,
    /// The client handles `variables` paging.
    pub supports_variable_paging: bool,
    /// The client implements the `runInTerminal` reverse request.
    pub supports_run_in_terminal_request: bool,
    /// The client implements the `startDebugging` reverse request.
    pub supports_start_debugging_request: bool,
}

impl InitializeRequestArguments {
    /// Arcode's arguments for the given adapter id.
    pub fn new(adapter_id: impl Into<String>) -> Self {
        InitializeRequestArguments {
            client_id: "arcode".into(),
            client_name: "Arcode".into(),
            adapter_id: adapter_id.into(),
            locale: "en-US".into(),
            lines_start_at1: true,
            columns_start_at1: true,
            path_format: "path".into(),
            supports_variable_type: true,
            supports_variable_paging: false,
            supports_run_in_terminal_request: true,
            supports_start_debugging_request: false,
        }
    }
}

/// A source file (or adapter-provided source).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// Short display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// File system path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Non-zero when the content must be fetched with the `source` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<i64>,
}

/// A breakpoint the client asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Break only when this expression is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Break only after this many hits (adapter-specific syntax).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Log this message (with `{expr}` interpolation) instead of breaking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

impl SourceBreakpoint {
    /// A plain breakpoint at a 1-based line.
    pub fn at_line(line: u32) -> Self {
        SourceBreakpoint {
            line,
            ..Default::default()
        }
    }
}

/// A breakpoint as the adapter reports it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    /// Adapter-assigned id (used by `breakpoint` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// Whether the breakpoint could be set.
    #[serde(default)]
    pub verified: bool,
    /// Why it is not verified, or other information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Source the breakpoint is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Actual 1-based line (may differ from the requested one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Actual 1-based column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// Arguments of `setBreakpoints`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArguments {
    /// The file.
    pub source: Source,
    /// The complete set of breakpoints for the file.
    pub breakpoints: Vec<SourceBreakpoint>,
}

/// Body of the `setBreakpoints` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetBreakpointsResponseBody {
    /// One entry per requested breakpoint, in order.
    #[serde(default)]
    pub breakpoints: Vec<Breakpoint>,
}

/// A breakpoint on a function name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpoint {
    /// Function name.
    pub name: String,
    /// Break only when this expression is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Break only after this many hits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

/// Arguments of `setFunctionBreakpoints`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetFunctionBreakpointsArguments {
    /// The complete set of function breakpoints.
    pub breakpoints: Vec<FunctionBreakpoint>,
}

/// Arguments of `setExceptionBreakpoints`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetExceptionBreakpointsArguments {
    /// Enabled filter ids (from `Capabilities::exception_breakpoint_filters`).
    pub filters: Vec<String>,
}

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
    use serde_json::json;

    #[test]
    fn initialize_arguments_wire_format() {
        let v = serde_json::to_value(InitializeRequestArguments::new("lldb")).unwrap();
        assert_eq!(v["clientID"], "arcode");
        assert_eq!(v["adapterID"], "lldb");
        assert_eq!(v["linesStartAt1"], true);
        assert_eq!(v["columnsStartAt1"], true);
        assert_eq!(v["pathFormat"], "path");
        assert_eq!(v["supportsVariableType"], true);
        assert_eq!(v["supportsRunInTerminalRequest"], true);
    }

    #[test]
    fn source_breakpoint_skips_absent_fields() {
        let bp = SourceBreakpoint {
            condition: Some("x > 1".into()),
            ..SourceBreakpoint::at_line(7)
        };
        assert_eq!(
            serde_json::to_value(&bp).unwrap(),
            json!({"line": 7, "condition": "x > 1"})
        );
        let log = SourceBreakpoint {
            hit_condition: Some("3".into()),
            log_message: Some("x={x}".into()),
            column: Some(2),
            ..SourceBreakpoint::at_line(1)
        };
        let v = serde_json::to_value(&log).unwrap();
        assert_eq!(v["hitCondition"], "3");
        assert_eq!(v["logMessage"], "x={x}");
    }

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
