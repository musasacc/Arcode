//! Reviewer tests: each one demonstrates a defect found during review.
//! They are expected to FAIL until the corresponding issue is fixed.

use std::str::FromStr;
use std::time::Duration;

use arcode_lsp::lsp_types::Uri;
use arcode_lsp::transport::{read_message, write_message};
use arcode_lsp::uri::path_str_to_uri_string;
use arcode_lsp::{EventReceiver, EventSink, LspClient, LspEvent, ServerStatus};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader};

async fn recv_json<R: AsyncBufRead + Unpin>(r: &mut R) -> Value {
    let text = tokio::time::timeout(Duration::from_secs(10), read_message(r))
        .await
        .expect("timed out")
        .expect("read failed")
        .expect("eof");
    serde_json::from_str(&text).expect("json")
}

async fn send_json<W: AsyncWrite + Unpin>(w: &mut W, v: Value) {
    write_message(w, &v.to_string()).await.expect("write");
}

/// Collects every event that arrives within `window`.
async fn drain(rx: &EventReceiver, window: Duration) -> Vec<LspEvent> {
    let mut out = Vec::new();
    let _ = tokio::time::timeout(window, async {
        while let Ok(ev) = rx.recv().await {
            out.push(ev);
        }
    })
    .await;
    out
}

fn statuses(events: &[LspEvent]) -> Vec<ServerStatus> {
    events
        .iter()
        .filter_map(|e| match e {
            LspEvent::ServerStatus { status, .. } => Some(status.clone()),
            _ => None,
        })
        .collect()
}

/// A server may send `$/progress` as soon as it has answered `initialize`
/// (before it sees `initialized`). The read loop then emits `Busy` while the
/// initialize task emits `Running` afterwards, so the UI ends up showing
/// "Running" while the server is indexing, until that progress ends.
#[tokio::test]
async fn review_progress_right_after_initialize_is_not_masked_by_running() {
    let (client_w, server_r) = tokio::io::duplex(64 * 1024);
    let (mut server_w, client_r) = tokio::io::duplex(64 * 1024);
    let mut server_r = BufReader::new(server_r);
    let (events, rx) = EventSink::channel();
    let client = tokio::spawn(LspClient::from_transport(
        "fake",
        std::env::temp_dir(),
        client_r,
        client_w,
        None,
        events,
    ));
    let init = recv_json(&mut server_r).await;
    assert_eq!(init["method"], "initialize");
    send_json(
        &mut server_w,
        json!({ "jsonrpc": "2.0", "id": init["id"], "result": { "capabilities": {} } }),
    )
    .await;
    send_json(
        &mut server_w,
        json!({ "jsonrpc": "2.0", "method": "$/progress", "params": {
            "token": "idx", "value": { "kind": "begin", "title": "Indexing" } } }),
    )
    .await;
    let initialized = recv_json(&mut server_r).await;
    assert_eq!(initialized["method"], "initialized");
    let _client = client.await.expect("join").expect("initialize");
    let seen = statuses(&drain(&rx, Duration::from_millis(300)).await);
    assert_eq!(
        seen.last(),
        Some(&ServerStatus::Busy("Indexing".into())),
        "indexing is still in progress but the last status is {:?} (all: {seen:?})",
        seen.last()
    );
}

/// Server stderr is read with `lines()`, which fails on the first
/// non-UTF-8 byte (common on Windows code pages). Forwarding then stops and
/// the pipe's read end is dropped, so the server's next stderr write gets
/// EPIPE/SIGPIPE and the server dies.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_non_utf8_stderr_keeps_forwarding_and_server_alive() {
    use arcode_lsp::ServerSpec;
    let script = r#"
printf 'before \377 bad byte\n' >&2
read -r _
body='{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
printf 'Content-Length: %d\r\n\r\n%s' ${#body} "$body"
sleep 1
printf 'after line\n' >&2
sleep 30
"#;
    let mut spec = ServerSpec::new("sh-fake", "sh");
    spec.args = vec!["-c".into(), script.into()];
    let (events, rx) = EventSink::channel();
    let client = LspClient::start(spec, std::env::temp_dir(), events)
        .await
        .expect("start");
    let seen = drain(&rx, Duration::from_secs(3)).await;
    let alive = client.is_alive();
    client.kill();
    let logs: Vec<&str> = seen
        .iter()
        .filter_map(|e| match e {
            LspEvent::Log { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        alive,
        "server died after Arcode stopped draining its stderr; statuses: {:?}",
        statuses(&seen)
    );
    assert!(
        logs.iter().any(|m| m.contains("after line")),
        "stderr forwarding stopped after a non-UTF-8 line; logs: {logs:?}"
    );
}

/// On Windows `path_to_uri` builds the URI with `path_str_to_uri_string(..,
/// true)` and then calls `Uri::from_str(..).expect(..)`. UNC server names are
/// copied into the authority without encoding, so valid Windows paths such
/// as WebDAV shares (`\\host@SSL@443\DavWWWRoot`) or non-ASCII machine names
/// produce an invalid URI and `path_to_uri` panics.
#[test]
fn review_windows_unc_hosts_produce_valid_uris() {
    for p in [
        r"\\files.example.com@SSL@443\DavWWWRoot\a.rs",
        "\\\\B\u{fc}cher-PC\\share\\a.rs",
    ] {
        let s = path_str_to_uri_string(p, true);
        assert!(
            Uri::from_str(&s).is_ok(),
            "{p:?} -> {s:?} is not a valid URI; path_to_uri would panic on Windows"
        );
    }
}

/// `LspClient::start` emits `Running` before the manager's actor stores the
/// client in its slot, so a UI that reacts to `Running` by issuing a request
/// (e.g. inlay hints for the visible file) can get `NoServer`.
#[cfg(unix)]
#[test]
fn review_running_status_means_requests_are_routable() {
    use arcode_lsp::{LspManager, ServerConfig};
    use std::collections::HashMap;
    // Waits for the first header line of `initialize` before answering it.
    let script = r#"read -r _
body='{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"hoverProvider":true}}}'
printf 'Content-Length: %d\r\n\r\n%s' ${#body} "$body"
sleep 30"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.txt");
    let mut misses = 0;
    let rounds = 500;
    for round in 0..rounds {
        let mut cfg = ServerConfig::new("sh", "sh", &[]);
        cfg.args = vec!["-c".into(), script.into()];
        let langs = HashMap::from([("txt".to_string(), vec!["sh".to_string()])]);
        let (manager, rx) = LspManager::new(vec![cfg], langs);
        manager.open_document(&file, "txt", 1, "x");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "round {round}: no Running event; seen {seen:?}"
            );
            match rx.try_recv() {
                Ok(LspEvent::ServerStatus {
                    status: ServerStatus::Running,
                    ..
                }) => break,
                Ok(other) => seen.push(other),
                Err(_) => std::hint::spin_loop(),
            }
        }
        if manager.running_servers(&file).is_empty() {
            misses += 1;
        }
    }
    assert_eq!(
        misses, 0,
        "{misses}/{rounds} times the server was not routable right after Running"
    );
}
