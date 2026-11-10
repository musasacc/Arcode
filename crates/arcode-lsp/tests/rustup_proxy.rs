//! A rustup proxy without its component (it prints `Unknown binary` and
//! exits) fails as "not installed", not as a crash; so does a spec a
//! launch hook marked as missing.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use arcode_lsp::{EventSink, LspClient, LspError, ServerSpec, ServerStatus, is_not_installed};

/// Writes an executable shell script.
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

async fn failed_reason(spec: ServerSpec, root: &Path) -> String {
    let (events, rx) = EventSink::channel();
    let started = LspClient::start(spec, root.to_path_buf(), events).await;
    assert!(started.is_err(), "the server must not start");
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("no Failed status")
            .expect("event channel closed");
        if let arcode_lsp::LspEvent::ServerStatus {
            status: ServerStatus::Failed(reason),
            ..
        } = event
        {
            return reason;
        }
    }
}

#[tokio::test]
async fn proxy_without_component_is_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let proxy = dir.path().join("rust-analyzer");
    script(
        &proxy,
        "echo \"error: Unknown binary 'rust-analyzer' in official toolchain \
         '1.97.0-x86_64-unknown-linux-gnu'.\" >&2\nexit 1",
    );
    let spec = ServerSpec::new("rust-analyzer", proxy.to_string_lossy());
    let reason = failed_reason(spec, dir.path()).await;
    assert!(is_not_installed(&reason), "{reason}");
    assert!(
        reason.contains("rustup component add rust-analyzer"),
        "{reason}"
    );
    assert!(
        reason.contains("1.97.0-x86_64-unknown-linux-gnu"),
        "{reason}"
    );
}

#[tokio::test]
async fn other_early_exits_stay_failures() {
    let dir = tempfile::tempdir().unwrap();
    let crash = dir.path().join("crashing-server");
    script(&crash, "echo 'panicked at main.rs' >&2\nexit 101");
    let spec = ServerSpec::new("crash", crash.to_string_lossy());
    let reason = failed_reason(spec, dir.path()).await;
    assert!(!is_not_installed(&reason), "{reason}");
}

#[tokio::test]
async fn a_spec_marked_missing_is_never_run() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    let program = dir.path().join("server");
    script(&program, &format!("touch '{}'", marker.display()));
    let spec = ServerSpec {
        missing: Some("rust-analyzer is not installed in the Rust toolchain".into()),
        ..ServerSpec::new("rust-analyzer", program.to_string_lossy())
    };
    let (events, _rx) = EventSink::channel();
    let err = LspClient::start(spec.clone(), dir.path().to_path_buf(), events)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        LspError::NotFound("rust-analyzer is not installed in the Rust toolchain".into())
    );
    let reason = failed_reason(spec, dir.path()).await;
    assert!(is_not_installed(&reason), "{reason}");
    assert!(!marker.exists(), "the command ran");
}
