//! Adapter capabilities (the `initialize` response body).

use serde::{Deserialize, Serialize};

/// What a debug adapter supports. Only a subset is modelled; unknown fields
/// are ignored and missing ones default to `false`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    /// The adapter wants `configurationDone`.
    pub supports_configuration_done_request: bool,
    /// `setFunctionBreakpoints` is supported.
    pub supports_function_breakpoints: bool,
    /// `SourceBreakpoint::condition` is honoured.
    pub supports_conditional_breakpoints: bool,
    /// `SourceBreakpoint::hit_condition` is honoured.
    pub supports_hit_conditional_breakpoints: bool,
    /// `evaluate` with context `hover` is side-effect free.
    pub supports_evaluate_for_hovers: bool,
    /// Exception filters for `setExceptionBreakpoints`; `None` means the
    /// adapter does not support exception breakpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_breakpoint_filters: Option<Vec<ExceptionBreakpointsFilter>>,
    /// `stepBack` / `reverseContinue` are supported.
    pub supports_step_back: bool,
    /// `setVariable` is supported.
    pub supports_set_variable: bool,
    /// `restartFrame` is supported.
    pub supports_restart_frame: bool,
    /// `completions` is supported.
    pub supports_completions_request: bool,
    /// `restart` is supported (otherwise the client restarts by
    /// disconnecting and launching again).
    pub supports_restart_request: bool,
    /// `terminate` is supported (graceful stop of the debuggee).
    pub supports_terminate_request: bool,
    /// `SourceBreakpoint::log_message` is honoured.
    pub supports_log_points: bool,
    /// `terminateDebuggee` in `disconnect` is honoured.
    pub support_terminate_debuggee: bool,
    /// `exceptionInfo` is supported.
    pub supports_exception_info_request: bool,
    /// `loadedSources` is supported.
    pub supports_loaded_sources_request: bool,
    /// `cancel` is supported.
    pub supports_cancel_request: bool,
    /// `stackTrace` may be called with `startFrame`/`levels`.
    pub supports_delayed_stack_trace_loading: bool,
    /// `singleThread` in `continue` / step requests is honoured.
    pub supports_single_thread_execution_requests: bool,
}

/// One exception filter (e.g. "raised" / "uncaught").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpointsFilter {
    /// Id passed in `setExceptionBreakpoints`.
    pub filter: String,
    /// Display label.
    #[serde(default)]
    pub label: String,
    /// Longer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Enabled by default in the UI.
    #[serde(default)]
    pub default: bool,
}

impl Capabilities {
    /// Applies a `capabilities` event: fields present in `update` replace
    /// the current ones.
    pub fn merge_json(&mut self, update: &serde_json::Value) {
        let Ok(mut current) = serde_json::to_value(&*self) else {
            return;
        };
        if let (Some(cur), Some(upd)) = (current.as_object_mut(), update.as_object()) {
            for (k, v) in upd {
                cur.insert(k.clone(), v.clone());
            }
        }
        if let Ok(merged) = serde_json::from_value(current) {
            *self = merged;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_and_unknown_fields() {
        let caps: Capabilities = serde_json::from_value(json!({
            "supportsConfigurationDoneRequest": true,
            "exceptionBreakpointFilters": [{"filter": "raised", "label": "Raised"}],
            "supportsSomethingNew": true
        }))
        .unwrap();
        assert!(caps.supports_configuration_done_request);
        assert!(!caps.supports_terminate_request);
        assert_eq!(caps.exception_breakpoint_filters.unwrap()[0].filter, "raised");
        let empty: Capabilities = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, Capabilities::default());
    }

    #[test]
    fn merge_overrides_present_fields_only() {
        let mut caps = Capabilities {
            supports_configuration_done_request: true,
            ..Default::default()
        };
        caps.merge_json(&json!({"supportsRestartRequest": true}));
        assert!(caps.supports_restart_request);
        assert!(caps.supports_configuration_done_request);
    }
}
