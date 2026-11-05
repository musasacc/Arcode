//! One task per server instance: starts the client, then applies document
//! messages and requests in the order the manager sent them. Messages sent
//! before initialization completes are queued in the channel.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use lsp_types::Uri;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use super::config::ServerConfig;
use super::text::LazyText;
use crate::client::{LspClient, ServerSpec};
use crate::event::{EventSink, PositionEncoding};
use crate::sync::TextChange;

/// Adjusts a server's process spec right before it starts; see
/// [`super::LspManager::set_launch_hook`].
pub type LaunchHook = Arc<dyn Fn(&mut ServerSpec) + Send + Sync>;

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
        /// The changes in each encoding they were computed for; a server
        /// whose encoding is missing gets the full text.
        changes: Arc<[(PositionEncoding, Vec<TextChange>)]>,
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
    /// Set when the server failed to start or its actor ended.
    stopped: Arc<AtomicBool>,
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

    /// Starting or running: not failed, stopped or exited.
    pub(crate) fn is_usable(&self) -> bool {
        !self.stopped.load(Ordering::Relaxed)
            && !self.tx.is_closed()
            && self.client.get().is_none_or(|c| c.is_alive())
    }
}

/// How long a replacement waits for the instance it replaces to stop.
const REPLACE_WAIT: Duration = Duration::from_secs(8);

/// How an instance starts.
pub(crate) struct Start {
    pub(crate) events: EventSink,
    /// The server starts only once this fires (the instance it replaces
    /// stopped) or after [`REPLACE_WAIT`]; commands queue meanwhile.
    pub(crate) after: Option<oneshot::Receiver<()>>,
    pub(crate) hook: Option<LaunchHook>,
}

/// Spawns the actor for `config` at `root` on `runtime`.
pub(crate) fn spawn(
    runtime: &Handle,
    config: &ServerConfig,
    root: PathBuf,
    start: Start,
) -> Arc<ServerSlot> {
    let Start {
        events,
        after,
        hook,
    } = start;
    let (tx, rx) = mpsc::unbounded_channel();
    let client = Arc::new(OnceLock::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let slot = Arc::new(ServerSlot {
        name: config.name.clone(),
        root: root.clone(),
        tx,
        client: client.clone(),
        stopped: stopped.clone(),
    });
    let config = config.clone();
    runtime.spawn(async move {
        if let Some(after) = after {
            let _ = tokio::time::timeout(REPLACE_WAIT, after).await;
        }
        let spec = launch_spec(&config, hook).await;
        run(spec, root, events, rx, client, &stopped).await;
        stopped.store(true, Ordering::Relaxed);
    });
    slot
}

/// The process spec of `config`, adjusted by `hook` on a blocking thread.
async fn launch_spec(config: &ServerConfig, hook: Option<LaunchHook>) -> ServerSpec {
    let spec = config.spec();
    let Some(hook) = hook else {
        return spec;
    };
    let fallback = spec.clone();
    tokio::task::spawn_blocking(move || {
        let mut spec = spec;
        hook(&mut spec);
        spec
    })
    .await
    .unwrap_or(fallback)
}

async fn run(
    spec: ServerSpec,
    root: PathBuf,
    events: EventSink,
    mut rx: mpsc::UnboundedReceiver<Command>,
    slot: Arc<OnceLock<LspClient>>,
    stopped: &AtomicBool,
) {
    // The slot is filled before `Running` is emitted, so a UI reacting to
    // that event can route requests to this server right away.
    let name = spec.name.clone();
    let started = LspClient::start_then(spec, root, events, |client| {
        let _ = slot.set(client.clone());
    });
    let client = match started.await {
        Ok(client) => client,
        Err(e) => {
            log::warn!("[{name}] failed to start: {e}");
            stopped.store(true, Ordering::Relaxed);
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
                log::debug!("[{name}] shutdown: {e}");
            }
            let _ = ack.send(());
            break;
        }
        if let Err(e) = apply(&client, cmd) {
            log::debug!("[{name}] {e}");
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
            full_text,
        } => {
            let own = changes
                .iter()
                .find(|(encoding, _)| *encoding == client.position_encoding())
                .map(|(_, changes)| changes.clone())
                .filter(|changes| !changes.is_empty());
            let Some(changes) = own else {
                // Ranges would be misinterpreted; send the whole text instead.
                return match client.document_sync().did_change_full(
                    uri,
                    version,
                    full_text.get().to_string(),
                ) {
                    Some(p) => client.notify::<lsp_types::notification::DidChangeTextDocument>(p),
                    None => Ok(()),
                };
            };
            client.did_change(uri, version, changes, || full_text.get().to_string())
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
