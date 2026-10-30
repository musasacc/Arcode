//! Event bodies and reverse-request arguments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Breakpoint;

/// Body of the `stopped` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedEventBody {
    /// `"step"`, `"breakpoint"`, `"exception"`, `"pause"`, `"entry"`, ...
    #[serde(default)]
    pub reason: String,
    /// Thread that stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<i64>,
    /// All threads stopped, not only `thread_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_stopped: Option<bool>,
    /// Full reason for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Extra text (e.g. the exception message).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Ids of the breakpoints that were hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_breakpoint_ids: Option<Vec<i64>>,
}

/// Body of the `continued` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuedEventBody {
    /// Thread that continued.
    #[serde(default)]
    pub thread_id: i64,
    /// Absent or true: all threads continued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

/// Body of the `exited` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitedEventBody {
    /// Exit code of the debuggee.
    #[serde(default)]
    pub exit_code: i64,
}

/// Body of the `terminated` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TerminatedEventBody {
    /// The adapter asks for a restart; passed back as `__restart` in the
    /// next launch arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<Value>,
}

/// Body of the `output` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputEventBody {
    /// `"console"`, `"important"`, `"stdout"`, `"stderr"`, `"telemetry"`, ...
    /// (absent means console).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The text (may contain partial lines).
    #[serde(default)]
    pub output: String,
}

/// Body of the `breakpoint` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BreakpointEventBody {
    /// `"changed"`, `"new"` or `"removed"`.
    #[serde(default)]
    pub reason: String,
    /// The breakpoint (matched by `id`).
    pub breakpoint: Breakpoint,
}

/// Body of the `thread` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventBody {
    /// `"started"` or `"exited"`.
    #[serde(default)]
    pub reason: String,
    /// The thread.
    pub thread_id: i64,
}

/// Body of the `process` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEventBody {
    /// Process name (usually the program path).
    #[serde(default)]
    pub name: String,
    /// OS process id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_process_id: Option<i64>,
    /// The process runs on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_local_process: Option<bool>,
    /// `"launch"`, `"attach"` or `"attachForSuspendedLaunch"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_method: Option<String>,
}

/// Arguments of the `runInTerminal` reverse request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInTerminalRequestArguments {
    /// `"integrated"` or `"external"` (Arcode always uses its own terminal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Terminal title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Working directory.
    #[serde(default)]
    pub cwd: String,
    /// Program and arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment changes; `None` values remove the variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, Option<String>>>,
    /// `args` may be joined into one shell command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_can_be_interpreted_by_shell: Option<bool>,
}

/// Body of the `runInTerminal` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInTerminalResponseBody {
    /// Process id of the started program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    /// Process id of the terminal shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_process_id: Option<u32>,
}

/// Arguments of the `startDebugging` reverse request (not supported yet).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartDebuggingRequestArguments {
    /// Launch/attach configuration of the child session.
    #[serde(default)]
    pub configuration: Value,
    /// `"launch"` or `"attach"`.
    #[serde(default)]
    pub request: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stopped_event() {
        let body: StoppedEventBody = serde_json::from_value(json!({
            "reason": "breakpoint", "threadId": 1, "allThreadsStopped": true,
            "hitBreakpointIds": [3]
        }))
        .unwrap();
        assert_eq!(body.reason, "breakpoint");
        assert_eq!(body.thread_id, Some(1));
        assert_eq!(body.hit_breakpoint_ids, Some(vec![3]));
    }

    #[test]
    fn run_in_terminal_env_allows_null() {
        let args: RunInTerminalRequestArguments = serde_json::from_value(json!({
            "kind": "integrated", "cwd": "/tmp", "args": ["python3", "x.py"],
            "env": {"A": "1", "B": null}
        }))
        .unwrap();
        let env = args.env.unwrap();
        assert_eq!(env["A"].as_deref(), Some("1"));
        assert_eq!(env["B"], None);
        let resp = RunInTerminalResponseBody {
            process_id: Some(42),
            shell_process_id: None,
        };
        assert_eq!(serde_json::to_value(resp).unwrap(), json!({"processId": 42}));
    }

    #[test]
    fn output_and_exited() {
        let out: OutputEventBody =
            serde_json::from_value(json!({"category": "stdout", "output": "hi\n"})).unwrap();
        assert_eq!(out.category.as_deref(), Some("stdout"));
        let exited: ExitedEventBody = serde_json::from_value(json!({"exitCode": 3})).unwrap();
        assert_eq!(exited.exit_code, 3);
    }
}
