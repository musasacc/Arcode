use super::*;
use serde_json::json;
use tokio::io::{BufReader, DuplexStream, ReadHalf, WriteHalf};

/// The adapter side of an in-memory connection.
struct Peer {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
}

impl Peer {
    async fn recv(&mut self) -> Value {
        let text = read_message(&mut self.reader).await.unwrap().unwrap();
        serde_json::from_str(&text).unwrap()
    }

    async fn send(&mut self, value: Value) {
        write_message(&mut self.writer, &value.to_string())
            .await
            .unwrap();
    }
}

fn pair() -> (
    Arc<Connection>,
    mpsc::UnboundedReceiver<Inbound>,
    Peer,
) {
    let (client, adapter) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (ar, aw) = tokio::io::split(adapter);
    let (conn, inbound) = Connection::spawn(BufReader::new(cr), cw, None);
    let peer = Peer {
        reader: BufReader::new(ar),
        writer: aw,
    };
    (conn, inbound, peer)
}

fn response(request_seq: i64, command: &str, body: Value) -> Value {
    json!({"seq": 100 + request_seq, "type": "response", "request_seq": request_seq,
        "success": true, "command": command, "body": body})
}

#[tokio::test]
async fn sequence_numbers_increase_and_responses_match_out_of_order() {
    let (conn, _inbound, mut peer) = pair();
    let a = conn.request("threads", None, None);
    let b = conn.request("scopes", Some(json!({"frameId": 1})), None);
    let ra = peer.recv().await;
    let rb = peer.recv().await;
    assert_eq!((ra["seq"].as_i64(), rb["seq"].as_i64()), (Some(1), Some(2)));
    assert_eq!(ra["type"], "request");
    assert_eq!(rb["arguments"]["frameId"], 1);
    peer.send(response(2, "scopes", json!({"scopes": []}))).await;
    peer.send(response(1, "threads", json!({"threads": []})))
        .await;
    assert_eq!(b.await.unwrap(), Some(json!({"scopes": []})));
    assert_eq!(a.await.unwrap(), Some(json!({"threads": []})));
}

#[tokio::test]
async fn failed_response_is_adapter_error() {
    let (conn, _inbound, mut peer) = pair();
    let fut = conn.request("evaluate", None, None);
    let req = peer.recv().await;
    peer.send(json!({"seq": 9, "type": "response", "request_seq": req["seq"],
        "success": false, "command": "evaluate", "message": "notStopped"}))
        .await;
    assert_eq!(
        fut.await,
        Err(DapError::Adapter {
            command: "evaluate".into(),
            message: "notStopped".into()
        })
    );
}

#[tokio::test]
async fn events_and_reverse_requests_are_forwarded_in_order_then_closed() {
    let (_conn, mut inbound, mut peer) = pair();
    peer.send(json!({"seq": 1, "type": "event", "event": "initialized"}))
        .await;
    peer.send(json!({"seq": 2, "type": "request", "command": "runInTerminal",
        "arguments": {"cwd": "/", "args": []}}))
        .await;
    drop(peer);
    assert!(matches!(inbound.recv().await, Some(Inbound::Event(e)) if e.event == "initialized"));
    assert!(
        matches!(inbound.recv().await, Some(Inbound::Request(r)) if r.command == "runInTerminal")
    );
    assert!(matches!(inbound.recv().await, Some(Inbound::Closed(_))));
}

#[tokio::test]
async fn adapter_death_fails_pending_and_later_requests() {
    let (conn, _inbound, mut peer) = pair();
    let pending = conn.request("continue", None, None);
    let _ = peer.recv().await;
    drop(peer);
    assert!(matches!(pending.await, Err(DapError::SessionEnded(_))));
    conn.closed().await;
    assert!(!conn.is_alive());
    let later = conn.request("threads", None, None).await;
    assert!(matches!(later, Err(DapError::SessionEnded(r)) if r.contains("closed")));
}

#[tokio::test]
async fn timeout_forgets_the_request() {
    let (conn, _inbound, mut peer) = pair();
    let fut = conn.request("threads", None, Some(Duration::from_millis(20)));
    let req = peer.recv().await;
    assert!(matches!(fut.await, Err(DapError::Timeout { .. })));
    // A late response is ignored.
    peer.send(response(req["seq"].as_i64().unwrap(), "threads", json!({})))
        .await;
    assert!(conn.lock().pending.is_empty());
}

#[tokio::test]
async fn responses_to_reverse_requests_use_the_next_seq() {
    let (conn, _inbound, mut peer) = pair();
    let _req = conn.request("initialize", None, None);
    let _ = peer.recv().await;
    conn.respond(7, "runInTerminal", Ok(Some(json!({"processId": 5}))))
        .unwrap();
    let msg = peer.recv().await;
    assert_eq!(msg["seq"], 2);
    assert_eq!(msg["type"], "response");
    assert_eq!(msg["request_seq"], 7);
    assert_eq!(msg["success"], true);
    assert_eq!(msg["body"]["processId"], 5);
    conn.respond(8, "startDebugging", Err("unsupported".into()))
        .unwrap();
    let msg = peer.recv().await;
    assert_eq!(msg["success"], false);
    assert_eq!(msg["message"], "unsupported");
}

#[tokio::test]
async fn malformed_messages_are_skipped() {
    let (conn, _inbound, mut peer) = pair();
    let fut = conn.request("threads", None, None);
    let _ = peer.recv().await;
    write_message(&mut peer.writer, "{not json").await.unwrap();
    peer.send(json!({"seq": 2, "type": "unknown"})).await;
    peer.send(response(1, "threads", json!({"threads": []})))
        .await;
    assert!(fut.await.is_ok());
}

#[tokio::test]
async fn kill_terminates_and_messages_queued_before_are_flushed() {
    let (conn, _inbound, mut peer) = pair();
    let fut = conn.request("disconnect", None, None);
    conn.kill("stopped by user");
    assert!(matches!(fut.await, Err(DapError::SessionEnded(r)) if r == "stopped by user"));
    let msg = peer.recv().await;
    assert_eq!(msg["command"], "disconnect");
}
