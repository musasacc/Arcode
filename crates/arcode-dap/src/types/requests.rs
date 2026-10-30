//! `initialize`, sources and breakpoints.

use serde::{Deserialize, Serialize};

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
}
