//! Reviewer tests: issues found while reviewing the crate. Each test is
//! expected to FAIL against the current implementation.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use arcode_dap::*;
use common::setup::{config, is_stopped};
use common::{Behavior, wait_event};

/// Rapid `set_breakpoints` calls for one file (toggle on/off quickly) must
/// reach the adapter in call order, otherwise the adapter ends up with a
/// stale set while `breakpoints()` reports the newest one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_set_breakpoints_reach_adapter_in_call_order() {
    let path = PathBuf::from("/work/src/lib.rs");
    for round in 0..40 {
        let (session, rx, received) = common::setup::start(Behavior::default(), config());
        wait_event(&rx, is_stopped).await;
        let mut futures = Vec::new();
        for line in 1..=20u32 {
            futures.push(session.set_breakpoints(&path, vec![SourceBreakpoint::at_line(line)]));
        }
        for f in futures {
            f.await.unwrap();
        }
        let lines: Vec<i64> = received
            .all()
            .iter()
            .filter(|m| m["command"] == "setBreakpoints")
            .filter(|m| m["arguments"]["source"]["path"] == "/work/src/lib.rs")
            .map(|m| m["arguments"]["breakpoints"][0]["line"].as_i64().unwrap())
            .collect();
        let expected: Vec<i64> = (1..=20).collect();
        assert_eq!(
            lines, expected,
            "round {round}: setBreakpoints sent out of call order (whenever the last two \
             swap, the adapter keeps a stale set while breakpoints() reports the newest)"
        );
    }
}

/// A stop during startup is a deliberate stop; it must not be reported as
/// `AdapterFailed` (the UI would show an error after pressing Stop).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_stop_during_startup_is_not_a_failure() {
    let mut failures = Vec::new();
    for round in 0..30 {
        let (client, adapter) = tokio::io::duplex(1 << 16);
        let (cr, cw) = tokio::io::split(client);
        let (ar, aw) = tokio::io::split(adapter);
        // Answers `initialize`, never answers `launch`, never sends
        // `initialized`; answers `disconnect` and closes.
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(ar);
            let mut writer = aw;
            let mut seq = 0;
            while let Ok(Some(text)) = transport::read_message(&mut reader).await {
                let req: serde_json::Value = serde_json::from_str(&text).unwrap();
                let command = req["command"].as_str().unwrap_or_default().to_string();
                if command == "initialize" || command == "disconnect" {
                    seq += 1;
                    let resp = serde_json::json!({"seq": seq, "type": "response",
                        "request_seq": req["seq"], "success": true, "command": command,
                        "body": {"supportsConfigurationDoneRequest": true}});
                    transport::write_message(&mut writer, &resp.to_string())
                        .await
                        .unwrap();
                }
                if command == "disconnect" {
                    return;
                }
            }
        });
        let mut cfg = config();
        cfg.breakpoints.clear();
        let (session, rx) = DebugSession::start_with_streams(cfg, cr, cw);
        // Let `initialize` complete and `launch` be pending.
        tokio::time::sleep(Duration::from_millis(50)).await;
        session.stop().await.unwrap();
        let end = common::collect_until(&rx, |e| {
            matches!(
                e,
                DebugEvent::AdapterExited(_) | DebugEvent::AdapterFailed(_)
            )
        })
        .await;
        if let Some(DebugEvent::AdapterFailed(m)) = end.last() {
            failures.push(format!("round {round}: {m}"));
        }
    }
    assert!(
        failures.is_empty(),
        "stop() during startup reported AdapterFailed: {failures:#?}"
    );
}

/// `connect_timeout` must bound the whole connect, including a single
/// attempt that hangs (SYNs dropped: unreachable host or a listener whose
/// accept queue is full). Uses only loopback.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn review_tcp_connect_timeout_bounds_a_hanging_attempt() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Fill the accept queue (never accept) so further SYNs are dropped.
    let mut fillers = Vec::new();
    for _ in 0..4096 {
        let attempt = tokio::time::timeout(
            Duration::from_millis(200),
            tokio::net::TcpStream::connect(("127.0.0.1", port)),
        )
        .await;
        match attempt {
            Ok(Ok(s)) => fillers.push(s),
            _ => break, // this attempt hung: the queue is full
        }
    }
    let spec = AdapterSpec {
        name: "remote".into(),
        adapter_id: "go".into(),
        transport: AdapterTransport::Tcp {
            command: None,
            host: "127.0.0.1".into(),
            port,
            connect_timeout: Duration::from_millis(300),
        },
        install_hint: None,
        configure_before_launch: false,
    };
    let started = std::time::Instant::now();
    let (_session, rx) = DebugSession::start(DebugConfig::new(
        spec,
        DebugRequest::Attach,
        serde_json::json!({}),
    ));
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(DebugEvent::AdapterFailed(m)) => return m,
                Ok(_) => {}
                Err(_) => panic!("channel closed"),
            }
        }
    })
    .await;
    drop(listener);
    assert!(
        outcome.is_ok(),
        "connect_timeout of 300 ms not enforced: no AdapterFailed after {:?}",
        started.elapsed()
    );
}

/// Dropping the last handle while a `restart()` is in flight must not
/// leave a freshly spawned adapter running with nobody to kill it.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn review_drop_during_restart_does_not_leak_adapter() {
    // A stdio "adapter" that never answers: the session stays in startup.
    let marker = "86399.4242";
    let spec = AdapterSpec {
        name: "sleeper".into(),
        adapter_id: "sleeper".into(),
        transport: AdapterTransport::Stdio {
            command: "sleep".into(),
            args: vec![marker.into()],
            env: vec![],
            cwd: None,
        },
        install_hint: None,
        configure_before_launch: false,
    };
    let (session, rx) = DebugSession::start(DebugConfig::new(
        spec,
        DebugRequest::Launch,
        serde_json::json!({}),
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let restart = session.restart();
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(session);
    let _ = tokio::time::timeout(Duration::from_secs(2), restart).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let alive = count_processes_with_arg(marker);
    // Drain events for the message.
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(format!("{e:?}"));
    }
    // Clean up whatever leaked.
    let _ = std::process::Command::new("pkill")
        .args(["-f", &format!("sleep {marker}")])
        .status();
    assert_eq!(
        alive, 0,
        "adapter process still running after the last handle was dropped; events: {events:#?}"
    );
}

#[cfg(target_os = "linux")]
fn count_processes_with_arg(arg: &str) -> usize {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    dir.filter_map(|e| e.ok())
        .filter_map(|e| std::fs::read(e.path().join("cmdline")).ok())
        .filter(|cmd| cmd.split(|b| *b == 0).any(|part| part == arg.as_bytes()))
        .count()
}

/// Breakpoints set before configuration are sent once with the startup
/// sequence; the handle's future must not send the same request again.
#[tokio::test]
async fn review_breakpoints_before_configuration_are_sent_once() {
    let (session, rx, received) = common::setup::start(
        Behavior {
            initialized_after_launch: true,
            ..Default::default()
        },
        DebugConfig::new(
            common::setup::dummy_spec(),
            DebugRequest::Attach,
            serde_json::json!({"pid": 1}),
        ),
    );
    let bps = session
        .set_breakpoints("/work/a.rs", vec![SourceBreakpoint::at_line(1)])
        .await
        .unwrap();
    assert_eq!(bps.len(), 1);
    wait_event(&rx, is_stopped).await;
    let sent = received
        .all()
        .iter()
        .filter(|m| m["command"] == "setBreakpoints")
        .filter(|m| m["arguments"]["source"]["path"] == "/work/a.rs")
        .count();
    assert_eq!(sent, 1, "setBreakpoints for /work/a.rs sent {sent} times");
}
