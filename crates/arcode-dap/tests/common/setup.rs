//! Shared session setup: a dummy adapter spec, a config with breakpoints,
//! and a session connected to the fake adapter over an in-memory stream.

use std::path::PathBuf;

use arcode_dap::*;
use serde_json::json;

use super::{Behavior, Received, run_fake};

pub fn dummy_spec() -> AdapterSpec {
    AdapterSpec {
        name: "fake".into(),
        adapter_id: "fake".into(),
        transport: AdapterTransport::Stdio {
            command: "fake-adapter".into(),
            args: vec![],
            env: vec![],
            cwd: None,
        },
        install_hint: None,
        configure_before_launch: false,
    }
}

pub fn config() -> DebugConfig {
    let mut config = DebugConfig::new(
        dummy_spec(),
        DebugRequest::Launch,
        json!({"program": "/work/app", "args": []}),
    );
    config.breakpoints.insert(
        PathBuf::from("/work/src/main.rs"),
        vec![
            SourceBreakpoint::at_line(3),
            SourceBreakpoint {
                condition: Some("x > 1".into()),
                ..SourceBreakpoint::at_line(7)
            },
        ],
    );
    config.function_breakpoints.push(FunctionBreakpoint {
        name: "app::run".into(),
        ..Default::default()
    });
    config
}

pub fn start(
    behavior: Behavior,
    config: DebugConfig,
) -> (DebugSessionHandle, EventReceiver, Received) {
    let (client, adapter) = tokio::io::duplex(1 << 16);
    let (cr, cw) = tokio::io::split(client);
    let (ar, aw) = tokio::io::split(adapter);
    let received = Received::default();
    tokio::spawn(run_fake(ar, aw, behavior, received.clone()));
    let (handle, rx) = DebugSession::start_with_streams(config, cr, cw);
    (handle, rx, received)
}

pub fn is_stopped(e: &DebugEvent) -> bool {
    matches!(e, DebugEvent::StateChanged(SessionState::Stopped { .. }))
}
