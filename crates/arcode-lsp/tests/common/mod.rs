//! In-process fake language server over `tokio::io::duplex`.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use arcode_lsp::transport::{read_message, write_message};
use arcode_lsp::{EventReceiver, EventSink, LspClient, LspEvent};
use serde_json::{Value, json};
use tokio::io::{BufReader, DuplexStream};

const STEP: Duration = Duration::from_secs(10);

/// The server side of a fake connection.
pub struct Peer {
    reader: BufReader<DuplexStream>,
    writer: DuplexStream,
}

impl Peer {
    /// Next message from the client, parsed (panics after a timeout or EOF).
    pub async fn recv(&mut self) -> Value {
        let text = tokio::time::timeout(STEP, read_message(&mut self.reader))
            .await
            .expect("timed out waiting for client message")
            .expect("read failed")
            .expect("client closed the connection");
        serde_json::from_str(&text).expect("client sent invalid JSON")
    }

    /// Next message, asserting its method.
    pub async fn expect(&mut self, method: &str) -> Value {
        let msg = self.recv().await;
        assert_eq!(msg["method"], method, "unexpected message: {msg}");
        msg
    }

    /// Returns `None` if the client closed the connection.
    pub async fn try_recv(&mut self) -> Option<Value> {
        let text = tokio::time::timeout(STEP, read_message(&mut self.reader))
            .await
            .ok()?
            .ok()??;
        serde_json::from_str(&text).ok()
    }

    /// Sends a raw JSON-RPC message to the client.
    pub async fn send(&mut self, msg: Value) {
        write_message(&mut self.writer, &msg.to_string())
            .await
            .expect("write failed");
    }

    /// Answers request `id` with `result`.
    pub async fn respond(&mut self, id: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await;
    }

    /// Sends a notification.
    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }
}

/// Default root used by the fake connections.
pub fn root() -> PathBuf {
    std::env::temp_dir().join("arcode-fake-root")
}

/// Connects a client to a fake server that answers `initialize` with
/// `capabilities`. Returns the client, the server side and the events.
pub async fn connect(
    capabilities: Value,
    init_options: Option<Value>,
) -> (LspClient, Peer, EventReceiver, Value) {
    let (client_w, server_r) = tokio::io::duplex(64 * 1024);
    let (server_w, client_r) = tokio::io::duplex(64 * 1024);
    let mut peer = Peer {
        reader: BufReader::new(server_r),
        writer: server_w,
    };
    let (events, rx) = EventSink::channel();
    let client = tokio::spawn(LspClient::from_transport(
        "fake",
        root(),
        client_r,
        client_w,
        init_options,
        events,
    ));
    let init = peer.expect("initialize").await;
    peer.respond(
        &init["id"],
        json!({ "capabilities": capabilities, "serverInfo": { "name": "fake", "version": "1" } }),
    )
    .await;
    peer.expect("initialized").await;
    let client = client.await.expect("join").expect("initialize failed");
    (client, peer, rx, init)
}

/// Waits for the first event matching `pred`, skipping others.
pub async fn next_event(rx: &EventReceiver, pred: impl Fn(&LspEvent) -> bool) -> LspEvent {
    tokio::time::timeout(STEP, async {
        loop {
            let ev = rx.recv().await.expect("event channel closed");
            if pred(&ev) {
                return ev;
            }
        }
    })
    .await
    .expect("timed out waiting for event")
}
