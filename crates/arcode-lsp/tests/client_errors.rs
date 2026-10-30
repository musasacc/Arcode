//! Request correlation, crashes, timeouts and cancellation.

mod common;

use std::time::Duration;

use arcode_lsp::lsp_types::WorkspaceSymbolParams;
use arcode_lsp::lsp_types::request::{Request, WorkspaceSymbolRequest};
use arcode_lsp::{LspError, LspEvent, ServerStatus};
use common::{connect, next_event};
use serde_json::json;

/// A tiny custom request so results are easy to tell apart.
enum Echo {}
impl Request for Echo {
    type Params = serde_json::Value;
    type Result = String;
    const METHOD: &'static str = "test/echo";
}

#[tokio::test]
async fn out_of_order_responses_are_correlated() {
    let (client, mut peer, _events, _) = connect(json!({}), None).await;
    let a = client.request::<Echo>(json!("a"));
    let b = client.request::<Echo>(json!("b"));
    let c = client.request::<Echo>(json!("c"));
    let mut received = Vec::new();
    for _ in 0..3 {
        let msg = peer.recv().await;
        assert_eq!(msg["method"], "test/echo");
        received.push(msg);
    }
    // Answer in reverse order, one of them with a string id.
    for msg in received.iter().rev() {
        let id = if msg["params"] == "b" {
            json!(msg["id"].to_string())
        } else {
            msg["id"].clone()
        };
        peer.respond(
            &id,
            json!(format!("answer-{}", msg["params"].as_str().unwrap())),
        )
        .await;
    }
    let (a, b, c) = tokio::join!(a, b, c);
    assert_eq!(a.unwrap(), "answer-a");
    assert_eq!(b.unwrap(), "answer-b");
    assert_eq!(c.unwrap(), "answer-c");
}

#[tokio::test]
async fn error_responses_become_rpc_errors() {
    let (client, mut peer, _events, _) = connect(json!({}), None).await;
    let fut = client.request::<Echo>(json!(1));
    let msg = peer.recv().await;
    peer.send(json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": -32801, "message": "modified" } }))
        .await;
    let err = fut.await.unwrap_err();
    assert!(err.is_cancellation(), "{err:?}");
    assert_eq!(
        err,
        LspError::Rpc {
            code: -32801,
            message: "modified".into()
        }
    );
}

#[tokio::test]
async fn server_crash_fails_pending_requests() {
    let (client, mut peer, events, _) = connect(json!({}), None).await;
    let first = client.request_with_timeout::<WorkspaceSymbolRequest>(
        WorkspaceSymbolParams {
            query: "x".into(),
            ..Default::default()
        },
        None,
    );
    let second = client.request_with_timeout::<Echo>(json!(2), None);
    peer.recv().await;
    peer.recv().await;
    drop(peer); // the "server" dies without answering
    let (first, second) = tokio::join!(first, second);
    assert!(
        matches!(first, Err(LspError::ServerStopped(_))),
        "{first:?}"
    );
    assert!(
        matches!(second, Err(LspError::ServerStopped(_))),
        "{second:?}"
    );
    let status = next_event(&events, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Failed(_),
                ..
            }
        )
    })
    .await;
    assert!(matches!(status, LspEvent::ServerStatus { ref server, .. } if server == "fake"));
    assert!(!client.is_alive());
    // New requests fail immediately instead of hanging.
    let late = client.request::<Echo>(json!(3)).await;
    assert!(matches!(late, Err(LspError::ServerStopped(_))));
    assert!(
        client
            .notify::<arcode_lsp::lsp_types::notification::Exit>(())
            .is_err()
    );
    // Shutdown of a dead server is a no-op.
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn timeout_sends_cancel_request() {
    let (client, mut peer, _events, _) = connect(json!({}), None).await;
    let fut = client.request_with_timeout::<Echo>(json!("slow"), Some(Duration::from_millis(50)));
    let (result, req) = tokio::join!(fut, peer.recv());
    assert_eq!(
        result.unwrap_err(),
        LspError::Timeout(Duration::from_millis(50))
    );
    let cancel = peer.expect("$/cancelRequest").await;
    assert_eq!(cancel["params"]["id"], req["id"]);
    // A late answer for the cancelled id is ignored and the client keeps working.
    peer.respond(&req["id"], json!("late")).await;
    let next = client.request::<Echo>(json!("next"));
    let server = async {
        let msg = peer.expect("test/echo").await;
        peer.respond(&msg["id"], json!("ok")).await;
    };
    let (next, ()) = tokio::join!(next, server);
    assert_eq!(next.unwrap(), "ok");
}

#[tokio::test]
async fn dropping_request_future_cancels() {
    let (client, mut peer, _events, _) = connect(json!({}), None).await;
    let fut = client.request::<Echo>(json!("dropped"));
    let req = peer.expect("test/echo").await;
    drop(fut);
    let cancel = peer.expect("$/cancelRequest").await;
    assert_eq!(cancel["params"]["id"], req["id"]);
}

#[tokio::test]
async fn invalid_json_from_server_is_logged_not_fatal() {
    let (client, mut peer, events, _) = connect(json!({}), None).await;
    peer.send(json!([1, 2, 3])).await;
    next_event(
        &events,
        |e| matches!(e, LspEvent::Log { message, .. } if message.contains("invalid")),
    )
    .await;
    assert!(client.is_alive());
}

#[tokio::test]
async fn failed_initialize_reports_failure() {
    let (client_w, server_r) = tokio::io::duplex(4096);
    let (server_w, client_r) = tokio::io::duplex(4096);
    let (events, rx) = arcode_lsp::EventSink::channel();
    drop((server_r, server_w)); // nobody answers
    let result = arcode_lsp::LspClient::from_transport(
        "dead",
        common::root(),
        client_r,
        client_w,
        None,
        events,
    )
    .await;
    assert!(result.is_err());
    next_event(&rx, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Failed(_),
                ..
            }
        )
    })
    .await;
}
