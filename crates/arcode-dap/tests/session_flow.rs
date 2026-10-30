//! A full session against the scripted fake adapter over an in-memory
//! stream: startup sequence, inspection, breakpoints, termination.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use arcode_dap::*;
use common::setup::{config, dummy_spec, is_stopped, start};
use common::{Behavior, collect_until, wait_event};
use serde_json::json;

async fn full_session(behavior: Behavior) {
    let (session, rx, received) = start(behavior, config());
    let events = collect_until(&rx, is_stopped).await;

    // Startup sequence on the wire.
    assert_eq!(
        received.commands(),
        [
            "initialize",
            "launch",
            "setBreakpoints",
            "setFunctionBreakpoints",
            "setExceptionBreakpoints",
            "configurationDone"
        ]
    );
    let init = &received.all()[0];
    assert_eq!(init["arguments"]["clientID"], "arcode");
    assert_eq!(init["arguments"]["adapterID"], "fake");
    assert_eq!(init["arguments"]["linesStartAt1"], true);
    assert_eq!(init["arguments"]["supportsRunInTerminalRequest"], true);
    let set_bps = &received.all()[2];
    assert_eq!(set_bps["arguments"]["source"]["path"], "/work/src/main.rs");
    assert_eq!(set_bps["arguments"]["breakpoints"][1]["condition"], "x > 1");
    assert_eq!(received.all()[4]["arguments"]["filters"], json!([]));

    // Events, in order.
    assert!(
        matches!(&events[0], DebugEvent::Capabilities(c) if c.supports_configuration_done_request)
    );
    assert!(events.iter().any(|e| matches!(e, DebugEvent::Initialized)));
    let set = events.iter().find_map(|e| match e {
        DebugEvent::BreakpointsSet { path, breakpoints } => Some((path, breakpoints)),
        _ => None,
    });
    let (path, bps) = set.expect("BreakpointsSet event");
    assert_eq!(path, &PathBuf::from("/work/src/main.rs"));
    assert_eq!(bps.len(), 2);
    assert!(bps.iter().all(|b| b.verified));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DebugEvent::Process(p) if p.system_process_id == Some(4242)))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DebugEvent::Output(o) if o.output == "hello\n"))
    );
    let stopped_at = events
        .iter()
        .position(|e| matches!(e, DebugEvent::Stopped(s) if s.reason == "breakpoint"))
        .expect("Stopped event");
    assert_eq!(stopped_at, events.len() - 2, "StateChanged follows Stopped");
    assert_eq!(
        session.state(),
        SessionState::Stopped {
            thread_id: Some(1),
            reason: "breakpoint".into()
        }
    );
    assert_eq!(
        session.known_threads(),
        [Thread {
            id: 1,
            name: "Thread 1".into()
        }]
    );

    // Inspection.
    let threads = session.threads().await.unwrap();
    assert_eq!(threads.len(), 2);
    assert_eq!(session.known_threads()[0].name, "main");
    let frames = session.stack_trace(1).await.unwrap();
    assert_eq!(frames[0].name, "app::main");
    assert_eq!(frames[0].line, 3);
    assert_eq!(
        frames[0].source.as_ref().and_then(|s| s.path.as_deref()),
        Some("/work/src/main.rs")
    );
    let scopes = session.scopes(frames[0].id).await.unwrap();
    assert_eq!(scopes[0].name, "Locals");
    assert!(scopes[1].expensive);
    let vars = session
        .variables(scopes[0].variables_reference)
        .await
        .unwrap();
    assert_eq!(vars[0].name, "x");
    assert_eq!(vars[0].type_.as_deref(), Some("i32"));
    let children = session
        .variables(vars[1].variables_reference)
        .await
        .unwrap();
    assert_eq!(children.len(), 2);
    let eval = session
        .evaluate("x", Some(frames[0].id), EvaluateContext::Hover)
        .await
        .unwrap();
    assert_eq!(eval.result, "x = 42");
    let eval_req = received
        .all()
        .into_iter()
        .find(|m| m["command"] == "evaluate")
        .unwrap();
    assert_eq!(eval_req["arguments"]["context"], "hover");
    assert_eq!(eval_req["arguments"]["frameId"], 1000);

    // Continue until the program exits.
    let resumed = session.continue_(1).await.unwrap();
    assert_eq!(resumed.all_threads_continued, Some(true));
    let tail = collect_until(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    assert!(
        tail.iter()
            .any(|e| matches!(e, DebugEvent::Exited(x) if x.exit_code == 0))
    );
    assert!(tail.iter().any(|e| matches!(e, DebugEvent::Terminated(_))));
    assert!(
        tail.iter()
            .any(|e| matches!(e, DebugEvent::StateChanged(SessionState::Terminated)))
    );
    assert_eq!(session.state(), SessionState::Terminated);
    assert_eq!(session.exit_code(), Some(0));
    // The client disconnected after `terminated`.
    assert_eq!(
        received.commands().last().map(String::as_str),
        Some("disconnect")
    );
    // Requests now fail instead of hanging.
    assert!(matches!(
        session.threads().await,
        Err(DapError::SessionEnded(_))
    ));
}

#[tokio::test]
async fn realistic_session_initialized_after_initialize() {
    full_session(Behavior::default()).await;
}

#[tokio::test]
async fn realistic_session_debugpy_style_launch_answered_after_configuration() {
    full_session(Behavior {
        launch_after_configuration_done: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn realistic_session_codelldb_style_initialized_after_launch() {
    full_session(Behavior {
        initialized_after_launch: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn evaluate_errors_carry_the_adapter_message() {
    let (session, rx, _) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    let err = session
        .evaluate("nope", None, EvaluateContext::Repl)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        DapError::Adapter {
            command: "evaluate".into(),
            message: "unknown name `nope`".into()
        }
    );
    // Arbitrary requests work too; unknown ones fail cleanly.
    let err = session
        .request("readMemory", Some(json!({})))
        .await
        .unwrap_err();
    assert!(matches!(err, DapError::Adapter { .. }));
}

#[tokio::test]
async fn breakpoints_can_change_during_the_session() {
    let (session, rx, received) = start(Behavior::default(), config());
    wait_event(&rx, is_stopped).await;
    let path = PathBuf::from("/work/src/lib.rs");
    let bps = session
        .set_breakpoints(
            &path,
            vec![
                SourceBreakpoint::at_line(10),
                SourceBreakpoint::at_line(999),
            ],
        )
        .await
        .unwrap();
    assert_eq!(bps.len(), 2);
    assert!(bps[0].verified && !bps[1].verified);
    let event = wait_event(
        &rx,
        |e| matches!(e, DebugEvent::BreakpointsSet { path: p, .. } if p == &path),
    )
    .await;
    assert!(
        matches!(event, DebugEvent::BreakpointsSet { breakpoints, .. } if breakpoints.len() == 2)
    );
    assert_eq!(session.breakpoints().len(), 2);
    // Clearing sends an empty list and forgets the file.
    let cleared = session.set_breakpoints(&path, vec![]).await.unwrap();
    assert!(cleared.is_empty());
    assert_eq!(session.breakpoints().len(), 1);
    let last = received
        .all()
        .into_iter()
        .rev()
        .find(|m| m["command"] == "setBreakpoints")
        .unwrap();
    assert_eq!(last["arguments"]["breakpoints"], json!([]));
    let fbps = session
        .set_function_breakpoints(vec![FunctionBreakpoint {
            name: "main".into(),
            ..Default::default()
        }])
        .await
        .unwrap();
    assert_eq!(fbps.len(), 1);
}

#[tokio::test]
async fn breakpoints_set_before_configuration_are_sent_with_startup() {
    let (session, rx, received) = start(
        Behavior {
            initialized_after_launch: true,
            ..Default::default()
        },
        DebugConfig::new(dummy_spec(), DebugRequest::Attach, json!({"pid": 1})),
    );
    // Called immediately, long before `initialized`.
    let fut = session.set_breakpoints("/work/a.rs", vec![SourceBreakpoint::at_line(1)]);
    let bps = tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bps.len(), 1);
    wait_event(&rx, is_stopped).await;
    let commands = received.commands();
    assert_eq!(commands[1], "attach");
    let first_set = commands.iter().position(|c| c == "setBreakpoints").unwrap();
    let initialized_sent_after = commands.iter().position(|c| c == "attach").unwrap();
    assert!(first_set > initialized_sent_after);
}
