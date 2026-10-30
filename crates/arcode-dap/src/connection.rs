//! One framed DAP connection: sequence numbers, pending requests, the
//! reader and writer tasks.
//!
//! Responses are matched to requests by `request_seq` (any order). Events
//! and reverse requests are forwarded, in wire order, to an [`Inbound`]
//! channel; [`Inbound::Closed`] is always the last item.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, watch};

use crate::error::{DapError, Result};
use crate::transport::{read_message, write_message};
use crate::types::{Event, ProtocolMessage, Request, Response};

/// Something the adapter sent that is not a response.
#[derive(Debug)]
pub(crate) enum Inbound {
    /// An event.
    Event(Event),
    /// A reverse request (`runInTerminal`, `startDebugging`, ...).
    Request(Request),
    /// The connection terminated; nothing follows.
    Closed(String),
}

type PendingSender = oneshot::Sender<Result<Option<Value>>>;

struct State {
    alive: bool,
    reason: String,
    next_seq: i64,
    pending: HashMap<i64, (String, PendingSender)>,
}

/// Shared state of one connection.
pub(crate) struct Connection {
    state: Mutex<State>,
    outgoing: mpsc::UnboundedSender<String>,
    inbound: mpsc::UnboundedSender<Inbound>,
    closed: watch::Sender<bool>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
}

impl Connection {
    /// Starts reader and writer tasks on the current tokio runtime. `kill`
    /// (if any) is fired, or dropped, to kill the adapter process.
    pub(crate) fn spawn<R, W>(
        reader: R,
        writer: W,
        kill: Option<oneshot::Sender<()>>,
    ) -> (Arc<Connection>, mpsc::UnboundedReceiver<Inbound>)
    where
        R: AsyncBufRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let conn = Arc::new(Connection {
            state: Mutex::new(State {
                alive: true,
                reason: String::new(),
                next_seq: 1,
                pending: HashMap::new(),
            }),
            outgoing: out_tx,
            inbound: in_tx,
            closed: watch::channel(false).0,
            kill: Mutex::new(kill),
        });
        tokio::spawn(read_loop(conn.clone(), reader));
        tokio::spawn(write_loop(conn.clone(), out_rx, writer));
        (conn, in_rx)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True until the connection terminates.
    pub(crate) fn is_alive(&self) -> bool {
        self.lock().alive
    }

    fn ended_error(state: &State) -> DapError {
        DapError::SessionEnded(if state.reason.is_empty() {
            "adapter not running".into()
        } else {
            state.reason.clone()
        })
    }

    /// Sends a request now and returns a future for its body. The future
    /// fails with [`DapError::SessionEnded`] if the connection dies first
    /// and with [`DapError::Timeout`] after `timeout` (polled inside tokio).
    pub(crate) fn request(
        self: &Arc<Self>,
        command: &str,
        arguments: Option<Value>,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Option<Value>>> + Send + use<> {
        let (tx, rx) = oneshot::channel();
        let seq = self.enqueue(command, arguments, tx);
        let conn = self.clone();
        let command = command.to_string();
        async move {
            let wait = async {
                rx.await
                    .unwrap_or_else(|_| Err(DapError::SessionEnded("connection dropped".into())))
            };
            match timeout {
                None => wait.await,
                Some(after) => match tokio::time::timeout(after, wait).await {
                    Ok(result) => result,
                    Err(_) => {
                        if let Some(seq) = seq {
                            conn.lock().pending.remove(&seq);
                        }
                        Err(DapError::Timeout { command, after })
                    }
                },
            }
        }
    }

    /// Registers a pending request and queues it; returns its `seq`, or
    /// `None` after resolving `tx` with an error.
    fn enqueue(&self, command: &str, arguments: Option<Value>, tx: PendingSender) -> Option<i64> {
        let mut state = self.lock();
        if !state.alive {
            let _ = tx.send(Err(Self::ended_error(&state)));
            return None;
        }
        let seq = state.next_seq;
        let message = ProtocolMessage::Request(Request {
            seq,
            command: command.to_string(),
            arguments,
        });
        let text = match serde_json::to_string(&message) {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(Err(e.into()));
                return None;
            }
        };
        state.next_seq += 1;
        // Queued under the lock so `seq` order equals wire order.
        if self.outgoing.send(text).is_err() {
            let _ = tx.send(Err(DapError::SessionEnded("writer closed".into())));
            return None;
        }
        state.pending.insert(seq, (command.to_string(), tx));
        Some(seq)
    }

    /// Answers a reverse request.
    pub(crate) fn respond(
        &self,
        request_seq: i64,
        command: &str,
        result: std::result::Result<Option<Value>, String>,
    ) -> Result<()> {
        let mut state = self.lock();
        if !state.alive {
            return Err(Self::ended_error(&state));
        }
        let (success, message, body) = match result {
            Ok(body) => (true, None, body),
            Err(msg) => (
                false,
                Some(msg.clone()),
                Some(serde_json::json!({"error": {"id": 1, "format": msg}})),
            ),
        };
        let text = serde_json::to_string(&ProtocolMessage::Response(Response {
            seq: state.next_seq,
            request_seq,
            success,
            command: command.to_string(),
            message,
            body,
        }))?;
        state.next_seq += 1;
        self.outgoing
            .send(text)
            .map_err(|_| DapError::SessionEnded("writer closed".into()))
    }

    fn handle_response(&self, response: Response) {
        let entry = self.lock().pending.remove(&response.request_seq);
        let Some((command, tx)) = entry else {
            log::debug!("response to unknown request {}", response.request_seq);
            return;
        };
        let result = if response.success {
            Ok(response.body)
        } else {
            Err(DapError::Adapter {
                command,
                message: response.error_text(),
            })
        };
        let _ = tx.send(result);
    }

    /// Marks the connection dead, fails all pending requests, stops the
    /// writer and reports [`Inbound::Closed`]. Only the first call counts.
    pub(crate) fn terminate(&self, reason: impl Into<String>) {
        let pending = {
            let mut state = self.lock();
            if !state.alive {
                return;
            }
            state.alive = false;
            state.reason = reason.into();
            let error = Self::ended_error(&state);
            let drained: Vec<_> = state.pending.drain().map(|(_, (_, tx))| tx).collect();
            let _ = self.inbound.send(Inbound::Closed(state.reason.clone()));
            (drained, error)
        };
        for tx in pending.0 {
            let _ = tx.send(Err(pending.1.clone()));
        }
        self.closed.send_replace(true);
    }

    /// Resolves once the connection has terminated.
    pub(crate) fn closed(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.closed.subscribe();
        async move {
            let _ = rx.wait_for(|closed| *closed).await;
        }
    }

    /// Kills the adapter process (if this connection owns one) and
    /// terminates the connection.
    pub(crate) fn kill(&self, reason: &str) {
        let tx = self.kill.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
        self.terminate(reason);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Dropping `kill` makes the supervisor kill the process.
        self.terminate("connection dropped");
    }
}

async fn read_loop<R>(conn: Arc<Connection>, mut reader: R)
where
    R: AsyncBufRead + Unpin + Send,
{
    let reason = loop {
        let text = tokio::select! {
            r = read_message(&mut reader) => r,
            _ = conn.closed() => return,
        };
        let text = match text {
            Ok(Some(text)) => text,
            Ok(None) => break "adapter closed the connection".to_string(),
            Err(e) => break format!("reading from adapter failed: {e}"),
        };
        match serde_json::from_str::<ProtocolMessage>(&text) {
            Ok(ProtocolMessage::Response(r)) => conn.handle_response(r),
            Ok(ProtocolMessage::Event(e)) => {
                let _ = conn.inbound.send(Inbound::Event(e));
            }
            Ok(ProtocolMessage::Request(r)) => {
                let _ = conn.inbound.send(Inbound::Request(r));
            }
            Err(e) => log::warn!("ignoring malformed adapter message ({e}): {text}"),
        }
    };
    conn.terminate(reason);
}

async fn write_loop<W>(
    conn: Arc<Connection>,
    mut rx: mpsc::UnboundedReceiver<String>,
    mut writer: W,
) where
    W: AsyncWrite + Unpin + Send,
{
    let closed = conn.closed();
    tokio::pin!(closed);
    loop {
        let message = tokio::select! {
            biased;
            m = rx.recv() => m,
            _ = &mut closed => None,
        };
        let Some(message) = message else { break };
        log::trace!("--> {message}");
        if let Err(e) = write_message(&mut writer, &message).await {
            conn.terminate(format!("writing to adapter failed: {e}"));
            break;
        }
    }
    // Flush whatever was queued before termination (e.g. `disconnect`).
    while let Ok(message) = rx.try_recv() {
        if write_message(&mut writer, &message).await.is_err() {
            break;
        }
    }
    let _ = tokio::io::AsyncWriteExt::shutdown(&mut writer).await;
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
