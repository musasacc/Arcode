//! One task per server instance: starts the client, then applies document
//! messages and requests in the order the manager sent them. Messages sent
//! before initialization completes are queued in the channel.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use lsp_types::Uri;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use super::config::ServerConfig;
use super::text::LazyText;
use crate::client::LspClient;
use crate::event::{EventSink, PositionEncoding};
use crate::sync::TextChange;

/// Produces a document's text for `didOpen`.
pub(crate) type MakeOpenText = dyn Fn() -> String + Send + Sync;

/// Work queued for a server.
pub(crate) enum Command {
    Open {
        uri: Uri,
        language_id: String,
        version: i32,
        /// Called when the notification is sent; the text is dropped after.
        text: Arc<MakeOpenText>,
    },
    Change {
        uri: Uri,
        version: i32,
        changes: Vec<TextChange>,
        encoding: PositionEncoding,
        /// Only built if this server needs the whole text.
        full_text: Arc<LazyText>,
    },
    Save {
        uri: Uri,
        text: Arc<LazyText>,
    },
    Close {
        uri: Uri,
    },
    /// Runs with the initialized client (used for requests).
    Run(Box<dyn FnOnce(&LspClient) + Send>),
    Shutdown(oneshot::Sender<()>),
}

/// Manager-side view of one server instance.
pub(crate) struct ServerSlot {
    pub(crate) name: String,
    pub(crate) root: PathBuf,
    tx: mpsc::UnboundedSender<Command>,
    client: Arc<OnceLock<LspClient>>,
}

impl ServerSlot {
    /// Queues a command; silently dropped if the actor is gone.
    pub(crate) fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    /// The client, if initialized and still alive.
    pub(crate) fn running_client(&self) -> Option<&LspClient> {
        self.client.get().filter(|c| c.is_alive())
    }
}

/// Spawns the actor for `config` at `root` on `runtime`.
pub(crate) fn spawn(
    runtime: &Handle,
    config: &ServerConfig,
    root: PathBuf,
    events: EventSink,
) -> Arc<ServerSlot> {
    let (tx, rx) = mpsc::unbounded_channel();
    let client = Arc::new(OnceLock::new());
    let slot = Arc::new(ServerSlot {
        name: config.name.clone(),
        root: root.clone(),
        tx,
        client: client.clone(),
    });
    runtime.spawn(run(config.clone(), root, events, rx, client));
    slot
}

async fn run(
    config: ServerConfig,
    root: PathBuf,
    events: EventSink,
    mut rx: mpsc::UnboundedReceiver<Command>,
    slot: Arc<OnceLock<LspClient>>,
) {
    // The slot is filled before `Running` is emitted, so a UI reacting to
    // that event can route requests to this server right away.
    let started = LspClient::start_then(config.spec(), root, events, |client| {
        let _ = slot.set(client.clone());
    });
    let client = match started.await {
        Ok(client) => client,
        Err(e) => {
            log::warn!("[{}] failed to start: {e}", config.name);
            // Keep draining so queued requests resolve (their senders drop).
            while let Some(cmd) = rx.recv().await {
                if let Command::Shutdown(ack) = cmd {
                    let _ = ack.send(());
                }
            }
            return;
        }
    };
    while let Some(cmd) = rx.recv().await {
        if let Command::Shutdown(ack) = cmd {
            if let Err(e) = client.shutdown().await {
                log::debug!("[{}] shutdown: {e}", config.name);
            }
            let _ = ack.send(());
            break;
        }
        if let Err(e) = apply(&client, cmd) {
            log::debug!("[{}] {e}", config.name);
        }
    }
}

fn apply(client: &LspClient, cmd: Command) -> crate::Result<()> {
    match cmd {
        Command::Open {
            uri,
            language_id,
            version,
            text,
        } => client.did_open(uri, &language_id, version, text()),
        Command::Change {
            uri,
            version,
            changes,
            encoding,
            full_text,
        } => {
            if changes.is_empty() || encoding != client.position_encoding() {
                // Ranges would be misinterpreted; send the whole text instead.
                match client.document_sync().did_change_full(
                    uri,
                    version,
                    full_text.get().to_string(),
                ) {
                    Some(p) => client.notify::<lsp_types::notification::DidChangeTextDocument>(p),
                    None => Ok(()),
                }
            } else {
                client.did_change(uri, version, changes, || full_text.get().to_string())
            }
        }
        Command::Save { uri, text } => client.did_save(uri, || text.get().to_string()),
        Command::Close { uri } => client.did_close(uri),
        Command::Run(f) => {
            f(client);
            Ok(())
        }
        Command::Shutdown(_) => Ok(()),
    }
}
