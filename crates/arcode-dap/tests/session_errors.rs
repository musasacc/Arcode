//! Out-of-order responses, adapter crashes, reverse requests, startup
//! failures and the TCP transport against the fake adapter.

mod common;

use std::time::Duration;

use arcode_dap::*;
use common::setup::{config, is_stopped};
use common::{Behavior, Received, collect_until, run_fake, wait_event};

fn spec() -> AdapterSpec {
    common::setup::dummy_spec()
}

fn start(behavior: Behavior) -> (DebugSessionHandle, EventReceiver, Received) {
    common::setup::start(behavior, config())
}

#[tokio::test]
async fn out_of_order_responses_resolve_the_right_futures() {
    let (session, rx, _) = start(Behavior {
        out_of_order: true,
        ..Default::default()
    });
    wait_event(&rx, is_stopped).await;
    // The first of these is answered only after the second.
    let frames = session.stack_trace(1);
    let scopes = session.scopes(1000);
    let (frames, scopes) = tokio::join!(frames, scopes);
    assert_eq!(frames.unwrap()[0].id, 1000);
    assert_eq!(scopes.unwrap()[0].name, "Locals");
    // Later concurrent requests keep working.
    let scopes = session.scopes(1000);
    let threads = session.threads();
    let frames = session.stack_trace(1);
    let (s, t, f) = tokio::join!(scopes, threads, frames);
    assert_eq!(s.unwrap().len(), 2);
    assert_eq!(t.unwrap().len(), 2);
    assert_eq!(f.unwrap().len(), 2);
}

#[tokio::test]
async fn adapter_crash_fails_pending_requests() {
    let (session, rx, _) = start(Behavior {
        crash_on: Some("stackTrace"),
        ..Default::default()
    });
    wait_event(&rx, is_stopped).await;
    let pending = session.stack_trace(1);
    let result = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .expect("pending request must not hang");
    assert!(
        matches!(result, Err(DapError::SessionEnded(_))),
        "{result:?}"
    );
    let events = collect_until(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    assert!(matches!(
        events.last(),
        Some(DebugEvent::AdapterExited(r)) if r.contains("closed")
    ));
    // The state changes before the final event is sent.
    assert!(matches!(
        &events[events.len() - 2],
        DebugEvent::StateChanged(SessionState::Terminated)
    ));
    assert_eq!(session.state(), SessionState::Terminated);
    let later = tokio::time::timeout(Duration::from_secs(5), session.variables(100))
        .await
        .unwrap();
    assert!(matches!(later, Err(DapError::SessionEnded(_))));
}

#[tokio::test]
async fn crash_during_startup_is_a_failure() {
    let (session, rx, _) = start(Behavior {
        crash_on: Some("launch"),
        ..Default::default()
    });
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
    // Depending on timing the read or the write side notices first.
    assert!(matches!(event, DebugEvent::AdapterFailed(m) if !m.is_empty()));
    assert_eq!(session.state(), SessionState::Terminated);
    // A request issued during startup errors out instead of hanging.
    let result = tokio::time::timeout(Duration::from_secs(5), session.threads())
        .await
        .unwrap();
    assert!(result.is_err());
    assert_eq!(session.state(), SessionState::Terminated);
}

#[tokio::test]
async fn launch_failure_is_reported_with_the_adapter_message() {
    let (session, rx, received) = start(Behavior {
        fail_launch: Some("no such program"),
        ..Default::default()
    });
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
    let DebugEvent::AdapterFailed(message) = event else {
        unreachable!()
    };
    assert_eq!(message, "launch failed: cannot launch: no such program");
    assert_eq!(session.state(), SessionState::Terminated);
    // `initialized` came before the launch error, so configuration was sent;
    // the session still ends and the adapter is disconnected or killed.
    assert_eq!(received.commands()[1], "launch");
    assert!(session.set_breakpoints("/x.rs", vec![]).await.is_err());
}

#[tokio::test]
async fn run_in_terminal_round_trip() {
    let (session, rx, received) = start(Behavior {
        run_in_terminal: true,
        ..Default::default()
    });
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::RunInTerminal(_))).await;
    let DebugEvent::RunInTerminal(request) = event else {
        unreachable!()
    };
    assert_eq!(request.arguments.args, ["/work/app", "--x"]);
    assert_eq!(request.arguments.cwd, "/work");
    let env = request.arguments.env.clone().unwrap();
    assert_eq!(env["B"], None);
    let clone = request.clone();
    request.respond(Some(31337), None).unwrap();
    assert!(clone.is_answered());
    assert!(clone.fail("again").is_err(), "answering twice is an error");
    wait_event(&rx, is_stopped).await;
    let responses = received.responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["command"], "runInTerminal");
    assert_eq!(responses[0]["success"], true);
    assert_eq!(responses[0]["body"]["processId"], 31337);
    drop(session);
}

#[tokio::test]
async fn unanswered_run_in_terminal_and_start_debugging_get_errors() {
    let (_session, rx, received) = start(Behavior {
        run_in_terminal: true,
        start_debugging: true,
        ..Default::default()
    });
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::RunInTerminal(_))).await;
    drop(event);
    wait_event(&rx, is_stopped).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while received.responses().len() < 2 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let responses = received.responses();
    assert_eq!(responses.len(), 2, "{responses:?}");
    for r in &responses {
        assert_eq!(r["success"], false, "{r}");
    }
    let start_debugging = responses
        .iter()
        .find(|r| r["command"] == "startDebugging")
        .unwrap();
    assert!(
        start_debugging["message"]
            .as_str()
            .unwrap()
            .contains("not supported")
    );
}

#[tokio::test]
async fn tcp_transport_to_a_listening_adapter() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Received::default();
    let fake_log = received.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (r, w) = stream.into_split();
        run_fake(r, w, Behavior::default(), fake_log).await;
    });
    let mut config = config();
    config.adapter.transport = AdapterTransport::Tcp {
        command: None,
        host: "127.0.0.1".into(),
        port,
        connect_timeout: Duration::from_secs(5),
    };
    let (session, rx) = DebugSession::start(config);
    wait_event(&rx, is_stopped).await;
    assert_eq!(session.stack_trace(1).await.unwrap().len(), 2);
    session.continue_(1).await.unwrap();
    wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    assert_eq!(session.exit_code(), Some(0));
    assert_eq!(received.commands()[0], "initialize");
}

#[tokio::test]
async fn missing_adapter_fails_with_install_hint() {
    let mut config = config();
    config.adapter = AdapterSpec {
        install_hint: Some("install the Rust language pack".into()),
        name: "codelldb".into(),
        transport: AdapterTransport::Tcp {
            command: Some(AdapterCommand {
                command: "arcode-no-such-codelldb".into(),
                args: vec!["--port".into(), PORT_PLACEHOLDER.into()],
                ..Default::default()
            }),
            host: "127.0.0.1".into(),
            port: 0,
            connect_timeout: Duration::from_secs(1),
        },
        ..spec()
    };
    let (session, rx) = DebugSession::start(config);
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
    assert!(matches!(
        event,
        DebugEvent::AdapterFailed(m) if m == "arcode-no-such-codelldb not found — install the Rust language pack"
    ));
    assert!(session.threads().await.is_err());
    // Restarting a failed session tries again (and fails the same way).
    assert!(session.restart().await.is_err());
}

#[tokio::test]
async fn tcp_connect_timeout_is_reported() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut config = config();
    config.adapter.transport = AdapterTransport::Tcp {
        command: None,
        host: "127.0.0.1".into(),
        port,
        connect_timeout: Duration::from_millis(200),
    };
    let (_session, rx) = DebugSession::start(config);
    let event = wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
    assert!(matches!(event, DebugEvent::AdapterFailed(m) if m.contains("could not connect")));
}
