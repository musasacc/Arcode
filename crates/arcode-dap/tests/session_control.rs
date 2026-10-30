//! Execution control against the fake adapter: stepping, pause, stop,
//! restart, dropping the handle, adapter quirks.

mod common;

use std::time::Duration;

use arcode_dap::*;
use common::setup::{config, is_stopped, start};
use common::{Behavior, collect_until, wait_event};

#[tokio::test]
async fn stepping_and_pause_update_the_state() {
    let (session, rx, received) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    for (step, name) in [(0, "next"), (1, "stepIn"), (2, "stepOut")] {
        match step {
            0 => session.next(1).await,
            1 => session.step_in(1).await,
            _ => session.step_out(1).await,
        }
        .unwrap();
        let events = collect_until(&rx, is_stopped).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DebugEvent::StateChanged(SessionState::Running))),
            "{name}: {events:?}"
        );
        assert!(
            matches!(&session.state(), SessionState::Stopped { reason, .. } if reason == "step")
        );
        assert_eq!(received.commands().last().unwrap(), name);
    }
    session.pause(1).await.unwrap();
    wait_event(&rx, is_stopped).await;
    assert!(matches!(&session.state(), SessionState::Stopped { reason, .. } if reason == "pause"));
}

#[tokio::test]
async fn stop_uses_terminate_when_supported() {
    let (session, rx, received) = start(
        Behavior {
            supports_terminate: true,
            ..Default::default()
        },
        config(),
    );
    wait_event(&rx, is_stopped).await;
    session.stop().await.unwrap();
    assert_eq!(session.state(), SessionState::Terminated);
    let commands = received.commands();
    let tail: Vec<&str> = commands
        .iter()
        .rev()
        .take(2)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(tail, ["terminate", "disconnect"]);
    wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
}

#[tokio::test]
async fn stop_disconnects_without_terminate_support() {
    let (session, rx, received) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    session.stop().await.unwrap();
    assert_eq!(session.state(), SessionState::Terminated);
    let disconnect = received.all().into_iter().last().unwrap();
    assert_eq!(disconnect["command"], "disconnect");
    assert_eq!(disconnect["arguments"]["terminateDebuggee"], true);
    assert!(!received.commands().contains(&"terminate".to_string()));
}

#[tokio::test]
async fn native_restart_reconfigures() {
    let (session, rx, received) = start(
        Behavior {
            supports_restart: true,
            ..Default::default()
        },
        config(),
    );
    wait_event(&rx, is_stopped).await;
    session.restart().await.unwrap();
    // The adapter sent `initialized` again: breakpoints and configurationDone follow.
    wait_event(&rx, is_stopped).await;
    let commands = received.commands();
    let restart = commands.iter().position(|c| c == "restart").unwrap();
    let after: Vec<&str> = commands[restart + 1..].iter().map(String::as_str).collect();
    assert!(
        after.contains(&"setBreakpoints") && after.contains(&"configurationDone"),
        "{after:?}"
    );
    let restart_req = received
        .all()
        .into_iter()
        .find(|m| m["command"] == "restart")
        .unwrap();
    assert_eq!(
        restart_req["arguments"]["arguments"]["program"],
        "/work/app"
    );
}

#[tokio::test]
async fn restart_of_a_stream_session_without_adapter_support_is_unsupported() {
    let (session, rx, _) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    assert!(matches!(
        session.restart().await,
        Err(DapError::Unsupported(_))
    ));
}

#[tokio::test]
async fn dropping_the_last_handle_closes_the_adapter() {
    let (session, rx, received) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    let clone = session.clone();
    drop(session);
    assert!(
        clone.threads().await.is_ok(),
        "a clone keeps the session alive"
    );
    drop(clone);
    wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    let before = received.all().len();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(received.all().len(), before);
}

#[tokio::test]
async fn configure_before_launch_quirk_sends_launch_last() {
    let mut config = config();
    config.adapter.configure_before_launch = true;
    let (session, rx, received) = start(Behavior::default(), config);
    wait_event(&rx, is_stopped).await;
    // The fake reports `stopped` at `configurationDone`, so `launch` may
    // still be in flight here.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while received.commands().len() < 6 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        received.commands(),
        [
            "initialize",
            "setBreakpoints",
            "setFunctionBreakpoints",
            "setExceptionBreakpoints",
            "configurationDone",
            "launch"
        ]
    );
    assert!(session.state().is_stopped());
}
