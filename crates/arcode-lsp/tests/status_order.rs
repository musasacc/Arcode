//! Server status events are totally ordered: progress, `initialize` and
//! termination never report states out of order.

mod common;

use std::time::Duration;

use arcode_lsp::transport::{read_message, write_message};
use arcode_lsp::{EventReceiver, EventSink, LspClient, LspEvent, ServerStatus};
use common::connect;
use serde_json::{Value, json};
use tokio::io::BufReader;

fn progress(token: &str, value: Value) -> Value {
    json!({ "token": token, "value": value })
}

fn drain_statuses(rx: &EventReceiver) -> Vec<ServerStatus> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            LspEvent::ServerStatus { status, .. } => Some(status),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn overlapping_progress_shows_oldest_title_then_running() {
    let (_client, mut peer, rx, _) = connect(json!({}), None).await;
    let begin = |t: &str| json!({ "kind": "begin", "title": t });
    let end = json!({ "kind": "end" });
    peer.notify("$/progress", progress("a", begin("Indexing")))
        .await;
    peer.notify("$/progress", progress("b", begin("Loading")))
        .await;
    peer.notify(
        "$/progress",
        progress("a", json!({ "kind": "report", "percentage": 50 })),
    )
    .await;
    peer.notify("$/progress", progress("a", end.clone())).await;
    peer.notify("$/progress", progress("b", end)).await;
    // `connect` consumed nothing from `rx`, so this is the full history.
    let mut seen = Vec::new();
    let collect = async {
        while seen.len() < 5 {
            if let LspEvent::ServerStatus { status, .. } = rx.recv().await.expect("closed") {
                seen.push(status);
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), collect)
        .await
        .unwrap_or_else(|_| panic!("too few status events: {seen:?}"));
    // Nothing else (e.g. a duplicate status) follows.
    tokio::time::sleep(Duration::from_millis(50)).await;
    seen.extend(drain_statuses(&rx));
    assert_eq!(
        seen,
        [
            ServerStatus::Starting,
            ServerStatus::Running,
            ServerStatus::Busy("Indexing".into()),
            ServerStatus::Busy("Loading".into()),
            ServerStatus::Running,
        ]
    );
}

/// A server that dies right after answering `initialize` may be reported
/// `Running` then `Failed`, or just `Failed` (with `start` failing), but
/// never `Running` after `Failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_after_initialize_never_reports_running_after_failed() {
    for _ in 0..200 {
        let (client_w, server_r) = tokio::io::duplex(64 * 1024);
        let (mut server_w, client_r) = tokio::io::duplex(64 * 1024);
        let (events, rx) = EventSink::channel();
        let client = tokio::spawn(LspClient::from_transport(
            "fake",
            std::env::temp_dir(),
            client_r,
            client_w,
            None,
            events,
        ));
        let mut server_r = BufReader::new(server_r);
        let init = read_message(&mut server_r).await.unwrap().unwrap();
        let init: Value = serde_json::from_str(&init).unwrap();
        let answer =
            json!({ "jsonrpc": "2.0", "id": init["id"], "result": { "capabilities": {} } });
        write_message(&mut server_w, &answer.to_string())
            .await
            .unwrap();
        drop(server_w);
        drop(server_r);
        let started = tokio::time::timeout(Duration::from_secs(10), client)
            .await
            .expect("initialize hung")
            .expect("join");
        // Collect statuses up to `Failed`, then a little longer to catch a
        // late `Running`.
        let mut seen = Vec::new();
        let collect = async {
            while let Ok(ev) = rx.recv().await {
                if let LspEvent::ServerStatus { status, .. } = ev {
                    let failed = matches!(status, ServerStatus::Failed(_));
                    seen.push(status);
                    if failed {
                        break;
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), collect)
            .await
            .expect("no Failed status after the server went away");
        tokio::time::sleep(Duration::from_millis(2)).await;
        seen.extend(drain_statuses(&rx));
        let failed = seen
            .iter()
            .position(|s| matches!(s, ServerStatus::Failed(_)))
            .expect("failed");
        assert!(
            !seen[failed..].iter().any(ServerStatus::is_running),
            "running reported after failure: {seen:?}"
        );
        match started {
            Ok(_) => assert!(seen[..failed].contains(&ServerStatus::Running), "{seen:?}"),
            Err(_) => assert!(!seen.iter().any(ServerStatus::is_running), "{seen:?}"),
        }
    }
}
