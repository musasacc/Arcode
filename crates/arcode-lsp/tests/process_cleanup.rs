//! Stopping a server also stops what it started: servers are often wrapper
//! scripts or launchers whose real server is a grandchild, and those must
//! not outlive the client (they used to, hundreds of them per CI run).
#![cfg(unix)]

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use arcode_lsp::{
    EventSink, LspClient, LspEvent, LspManager, ServerConfig, ServerSpec, ServerStatus,
};

/// A fake server that starts a long `sleep`, writes its pid to `pid_file`,
/// answers `initialize` once it sees the request's first header line and
/// then waits for the sleep.
fn wrapper_script(pid_file: &Path) -> String {
    format!(
        r#"sleep 30 & echo $! > '{}'
read -r _
body='{{"jsonrpc":"2.0","id":1,"result":{{"capabilities":{{}}}}}}'
printf 'Content-Length: %d\r\n\r\n%s' ${{#body}} "$body"
wait"#,
        pid_file.display()
    )
}

/// The pid the script wrote, once it is there.
fn grandchild(pid_file: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(pid) = std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|t| t.trim().parse().ok())
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "the fake server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `pid` runs (a zombie nobody reaped yet counts as stopped).
fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or_else(
        // No procfs (macOS): ask `kill -0`.
        |_| {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        },
        |stat| {
            !stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        },
    )
}

fn assert_stops(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) {
        assert!(
            Instant::now() < deadline,
            "the server's child {pid} outlived it"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_client_stops_the_servers_children() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("pid");
    let mut spec = ServerSpec::new("wrapper", "sh");
    spec.args = vec!["-c".into(), wrapper_script(&pid_file)];
    let (events, _rx) = EventSink::channel();
    let client = LspClient::start(spec, dir.path().to_path_buf(), events)
        .await
        .expect("start");
    let pid = grandchild(&pid_file);
    assert!(alive(pid));
    drop(client);
    tokio::task::spawn_blocking(move || assert_stops(pid))
        .await
        .expect("join");
}

#[test]
fn dropping_the_manager_stops_the_servers_children() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("pid");
    let mut cfg = ServerConfig::new("wrapper", "sh", &[]);
    cfg.args = vec!["-c".into(), wrapper_script(&pid_file)];
    let langs = HashMap::from([("txt".to_string(), vec!["wrapper".to_string()])]);
    let (manager, rx) = LspManager::new(vec![cfg], langs);
    manager.open_document(&dir.path().join("a.txt"), "txt", 1, "x");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "the server never ran");
        match rx.try_recv() {
            Ok(LspEvent::ServerStatus {
                status: ServerStatus::Running,
                ..
            }) => break,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let pid = grandchild(&pid_file);
    assert!(alive(pid));
    drop(manager);
    assert_stops(pid);
}
