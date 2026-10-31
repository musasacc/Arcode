//! Real child-process transports. This binary has its own `main`: with
//! `ARCODE_FAKE_DAP` set it *is* a fake adapter (on stdio, or listening on
//! `--port N`), otherwise it runs the tests, which spawn it.

mod common;

use std::time::Duration;

use arcode_dap::*;
use common::{Behavior, Received, collect_until, run_fake, wait_event};
use serde_json::json;

fn main() {
    match std::env::var("ARCODE_FAKE_DAP").as_deref() {
        Ok("stdio") => return fake_stdio(),
        Ok("tcp") => return fake_tcp(),
        Ok("die") => {
            eprintln!("Traceback (most recent call last):");
            eprintln!("ModuleNotFoundError: No module named 'debugpy'");
            std::process::exit(1);
        }
        _ => {}
    }
    let tests: [(&str, fn()); 5] = [
        ("stdio_process_session", stdio_process_session),
        (
            "tcp_spawned_adapter_with_port_placeholder",
            tcp_spawned_adapter,
        ),
        ("restart_respawns_the_adapter", restart_respawns_the_adapter),
        (
            "dying_adapter_reports_its_stderr",
            dying_adapter_reports_its_stderr,
        ),
        ("kill_ends_the_session", kill_ends_the_session),
    ];
    let filter: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let mut failed = 0;
    for (name, test) in tests {
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    if failed > 0 {
        eprintln!("{failed} test(s) failed");
        std::process::exit(1);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn fake_stdio() {
    eprintln!("fake adapter starting");
    runtime().block_on(run_fake(
        tokio::io::stdin(),
        tokio::io::stdout(),
        Behavior::default(),
        Received::default(),
    ));
}

fn fake_tcp() {
    let args: Vec<String> = std::env::args().collect();
    let port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse::<u16>().ok())
        .expect("--port N");
    println!("listening on {port}");
    runtime().block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (r, w) = stream.into_split();
        run_fake(
            r,
            w,
            Behavior {
                supports_terminate: true,
                ..Default::default()
            },
            Received::default(),
        )
        .await;
    });
}

fn exe() -> String {
    std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn stdio_config(mode: &str) -> DebugConfig {
    let spec = AdapterSpec {
        name: "fake".into(),
        adapter_id: "fake".into(),
        transport: AdapterTransport::Stdio {
            command: exe(),
            args: vec![],
            env: vec![("ARCODE_FAKE_DAP".into(), mode.into())],
            cwd: Some(std::env::temp_dir()),
        },
        install_hint: None,
        configure_before_launch: false,
    };
    let mut config = DebugConfig::new(spec, DebugRequest::Launch, json!({"program": "/work/app"}));
    config.breakpoints.insert(
        "/work/src/main.rs".into(),
        vec![SourceBreakpoint::at_line(3)],
    );
    config
}

fn is_stopped(e: &DebugEvent) -> bool {
    matches!(e, DebugEvent::StateChanged(SessionState::Stopped { .. }))
}

fn stdio_process_session() {
    runtime().block_on(async {
        let (session, rx) = DebugSession::start(stdio_config("stdio"));
        let events = collect_until(&rx, is_stopped).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DebugEvent::AdapterLog(l) if l == "fake adapter starting")),
            "{events:?}"
        );
        assert_eq!(session.stack_trace(1).await.unwrap()[0].name, "app::main");
        session.continue_(1).await.unwrap();
        wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
        assert_eq!(session.state(), SessionState::Terminated);
        assert_eq!(session.exit_code(), Some(0));
    });
}

fn tcp_spawned_adapter() {
    runtime().block_on(async {
        let mut config = stdio_config("tcp");
        config.adapter.transport = AdapterTransport::Tcp {
            command: Some(AdapterCommand {
                command: exe(),
                args: vec!["--port".into(), PORT_PLACEHOLDER.into()],
                env: vec![("ARCODE_FAKE_DAP".into(), "tcp".into())],
                cwd: None,
            }),
            host: "127.0.0.1".into(),
            port: 0,
            connect_timeout: Duration::from_secs(10),
        };
        let (session, rx) = DebugSession::start(config);
        let events = collect_until(&rx, is_stopped).await;
        // The adapter's stdout is a log stream for TCP adapters.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DebugEvent::AdapterLog(l) if l.starts_with("listening on "))),
            "{events:?}"
        );
        session.stop().await.unwrap();
        assert_eq!(session.state(), SessionState::Terminated);
        wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    });
}

fn restart_respawns_the_adapter() {
    runtime().block_on(async {
        let (session, rx) = DebugSession::start(stdio_config("stdio"));
        wait_event(&rx, is_stopped).await;
        let restarted = session.restart();
        let events = collect_until(&rx, is_stopped).await;
        restarted.await.unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DebugEvent::StateChanged(SessionState::Initializing))),
            "{events:?}"
        );
        let starts = events
            .iter()
            .filter(|e| matches!(e, DebugEvent::AdapterLog(l) if l == "fake adapter starting"))
            .count();
        assert_eq!(starts, 1, "a new adapter process was started: {events:?}");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DebugEvent::AdapterExited(_))),
            "the old adapter's exit is not reported: {events:?}"
        );
        assert_eq!(session.stack_trace(1).await.unwrap().len(), 2);
        session.stop().await.unwrap();
    });
}

fn dying_adapter_reports_its_stderr() {
    runtime().block_on(async {
        let (session, rx) = DebugSession::start(stdio_config("die"));
        let event = wait_event(&rx, |e| matches!(e, DebugEvent::AdapterFailed(_))).await;
        let DebugEvent::AdapterFailed(message) = event else {
            unreachable!()
        };
        assert!(message.contains("No module named 'debugpy'"), "{message}");
        assert_eq!(session.state(), SessionState::Terminated);
    });
}

fn kill_ends_the_session() {
    runtime().block_on(async {
        let (session, rx) = DebugSession::start(stdio_config("stdio"));
        wait_event(&rx, is_stopped).await;
        session.kill();
        wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
        assert_eq!(session.state(), SessionState::Terminated);
        let result = tokio::time::timeout(Duration::from_secs(5), session.threads())
            .await
            .unwrap();
        assert!(result.is_err());
    });
}
