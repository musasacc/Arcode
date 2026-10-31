//! Request ordering across handle calls and breakpoints set before the
//! session is configured.

mod common;

use std::path::PathBuf;

use arcode_dap::*;
use common::setup::{config, dummy_spec, is_stopped, start};
use common::{Behavior, wait_event};
use serde_json::json;

fn attach_config() -> DebugConfig {
    DebugConfig::new(dummy_spec(), DebugRequest::Attach, json!({"pid": 1}))
}

/// Mixed requests made without awaiting in between reach the adapter in
/// call order, even with several runtime workers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_requests_keep_call_order() {
    for _ in 0..20 {
        let (session, rx, received) = start(Behavior::default(), config());
        wait_event(&rx, is_stopped).await;
        let before = received.commands().len();
        let threads = session.threads();
        let stack = session.stack_trace(1);
        let scopes = session.scopes(1);
        let eval = session.evaluate("x", Some(1), EvaluateContext::Watch);
        let next = session.next(1);
        let (a, b, c, d, e) = tokio::join!(threads, stack, scopes, eval, next);
        let _ = (a, b, c, d);
        e.unwrap();
        let sent: Vec<String> = received.commands()[before..].to_vec();
        assert_eq!(
            sent,
            ["threads", "stackTrace", "scopes", "evaluate", "next"],
            "requests sent out of call order"
        );
    }
}

/// Function breakpoints stored before configuration go out once, with the
/// startup sequence, and the handle's future gets that answer.
#[tokio::test]
async fn function_breakpoints_before_configuration_are_sent_once() {
    let (session, rx, received) = start(
        Behavior {
            initialized_after_launch: true,
            ..Default::default()
        },
        attach_config(),
    );
    let bps = session
        .set_function_breakpoints(vec![
            FunctionBreakpoint {
                name: "main".into(),
                ..Default::default()
            },
            FunctionBreakpoint {
                name: "run".into(),
                ..Default::default()
            },
        ])
        .await
        .unwrap();
    assert_eq!(bps.len(), 2);
    wait_event(&rx, is_stopped).await;
    let sent = received
        .commands()
        .iter()
        .filter(|c| *c == "setFunctionBreakpoints")
        .count();
    assert_eq!(sent, 1);
}

/// Several changes to one file before configuration: the adapter ends up
/// with the newest set, every future resolves, and nothing is sent twice.
#[tokio::test]
async fn repeated_changes_before_configuration_send_the_newest_set() {
    let (session, rx, received) = start(
        Behavior {
            initialized_after_launch: true,
            ..Default::default()
        },
        attach_config(),
    );
    let path = PathBuf::from("/work/b.rs");
    let first = session.set_breakpoints(&path, vec![SourceBreakpoint::at_line(1)]);
    let second = session.set_breakpoints(&path, vec![SourceBreakpoint::at_line(2)]);
    let removed = session.set_breakpoints("/work/c.rs", vec![]);
    let (first, second, removed) = tokio::join!(first, second, removed);
    assert_eq!(first.unwrap()[0].line, Some(2));
    assert_eq!(second.unwrap()[0].line, Some(2));
    assert!(removed.unwrap().is_empty());
    wait_event(&rx, is_stopped).await;
    let lines: Vec<i64> = received
        .all()
        .iter()
        .filter(|m| m["command"] == "setBreakpoints")
        .filter(|m| m["arguments"]["source"]["path"] == "/work/b.rs")
        .filter_map(|m| m["arguments"]["breakpoints"][0]["line"].as_i64())
        .collect();
    assert_eq!(lines, [2]);
    assert_eq!(session.breakpoints().get(&path).map(|b| b[0].line), Some(2));
}

/// After configuration every call is sent (no stale "already sent" answer).
#[tokio::test]
async fn changes_after_configuration_are_always_sent() {
    let (session, rx, received) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    let path = "/work/src/main.rs";
    for line in [10, 11] {
        let bps = session
            .set_breakpoints(path, vec![SourceBreakpoint::at_line(line)])
            .await
            .unwrap();
        assert_eq!(bps[0].line, Some(line));
    }
    let lines: Vec<i64> = received
        .all()
        .iter()
        .filter(|m| m["command"] == "setBreakpoints")
        .filter(|m| m["arguments"]["source"]["path"] == path)
        .filter_map(|m| m["arguments"]["breakpoints"][0]["line"].as_i64())
        .collect();
    assert_eq!(lines, [3, 10, 11]);
}

/// Requests queued behind a session that dies fail instead of hanging.
#[tokio::test]
async fn queued_requests_fail_when_the_adapter_dies() {
    let (session, rx, _received) = start(
        Behavior {
            crash_on: Some("launch"),
            ..Default::default()
        },
        config(),
    );
    let bps = session.set_breakpoints("/work/x.rs", vec![SourceBreakpoint::at_line(1)]);
    let threads = session.threads();
    assert!(bps.await.is_err());
    assert!(threads.await.is_err());
    wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
}
