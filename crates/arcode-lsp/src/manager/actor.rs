//! One task per server instance: starts the client, then applies document
//! messages and requests in the order the manager sent them. Messages sent
//! before initialization completes are queued in the channel.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use lsp_types::Uri;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use super::config::ServerConfig;
use super::detached::{self, Added, DetachedFiles, FolderLimit, Launch, Update};
use super::text::LazyText;
use crate::client::{LspClient, ServerSpec};
use crate::event::{EventSink, LspEvent, PositionEncoding};
use crate::sync::TextChange;

/// Adjusts a server's process spec right before it starts in the given
/// root folder; see [`super::LspManager::set_launch_hook`].
pub type LaunchHook = Arc<dyn Fn(&mut ServerSpec, &Path) + Send + Sync>;

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
    /// A loose file was added: tell the server; see [`detached`].
    UpdateDetached,
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
    /// Loose Rust files it serves; see [`detached`].
    detached: Arc<DetachedFiles>,
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

    /// Adds a loose Rust file to the instance, telling the server if it
    /// already started without it; see [`detached`]. Send the file's
    /// `didOpen` first. False when the instance is retired: the file needs
    /// a new one.
    pub(crate) fn add_detached(&self, file: &Path) -> bool {
        match self.detached.add(file) {
            Added::New => self.send(Command::UpdateDetached),
            Added::Known => {}
            Added::Retired => return false,
        }
        true
    }

    /// The loose file `path` (or every file below the folder `path`) was
    /// closed, renamed or deleted. The server keeps it until its list
    /// changes for another reason.
    pub(crate) fn close_detached(&self, path: &Path) {
        self.detached.remove(path);
    }

    /// Whether the instance serves loose Rust files; see [`detached`].
    pub(crate) fn serves_loose_files(&self) -> bool {
        self.detached.is_loose()
    }

    /// Whether the server's list of loose files names a missing one; see
    /// [`detached`].
    pub(crate) fn detached_missing(&self) -> bool {
        self.detached.names_missing()
    }

    /// Never started because none of its loose files can be analysed: a
    /// file opened for it needs a new instance.
    pub(crate) fn is_retired(&self) -> bool {
        self.detached.is_retired()
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
    /// The loose Rust file it is started for, if any.
    pub(crate) loose: Option<PathBuf>,
    /// How big a loose file's folder may be.
    pub(crate) limit: FolderLimit,
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
        loose,
        limit,
    } = start;
    let (tx, rx) = mpsc::unbounded_channel();
    let client = Arc::new(OnceLock::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let options = config.initialization_options.clone();
    let detached = Arc::new(DetachedFiles::new(options, limit, loose.as_deref()));
    let slot = Arc::new(ServerSlot {
        name: config.name.clone(),
        root: root.clone(),
        tx,
        client: client.clone(),
        stopped: stopped.clone(),
        detached: detached.clone(),
    });
    let config = config.clone();
    runtime.spawn(async move {
        if let Some(after) = after {
            let _ = tokio::time::timeout(REPLACE_WAIT, after).await;
        }
        let mut spec = launch_spec(&config, &root, hook).await;
        let instance = Instance {
            name: config.name.clone(),
            events,
            client,
            detached,
        };
        match instance.launch().await {
            Launch::Configured => {}
            Launch::Detached(options) => spec.initialization_options = Some(options),
            Launch::Nothing => {
                log::info!("[{}] no loose file to analyse, not started", spec.name);
                stopped.store(true, Ordering::Relaxed);
                return;
            }
        }
        run(spec, root, instance, rx, &stopped).await;
        stopped.store(true, Ordering::Relaxed);
    });
    slot
}

/// What the actor shares with its [`ServerSlot`].
struct Instance {
    name: String,
    events: EventSink,
    client: Arc<OnceLock<LspClient>>,
    detached: Arc<DetachedFiles>,
}

impl Instance {
    /// How the server starts; counts loose files' folders off the runtime.
    async fn launch(&self) -> Launch {
        if !self.detached.is_loose() {
            return self.detached.launch().action;
        }
        let detached = self.detached.clone();
        match tokio::task::spawn_blocking(move || detached.launch()).await {
            Ok(outcome) => {
                self.refused(&outcome.refused);
                outcome.action
            }
            Err(_) => Launch::Nothing,
        }
    }

    /// Tells the running server about a loose file added to it.
    async fn update(&self, client: &LspClient) {
        let detached = self.detached.clone();
        let Ok(outcome) = tokio::task::spawn_blocking(move || detached.update()).await else {
            return;
        };
        self.refused(&outcome.refused);
        if let Update::Send(options) = outcome.action {
            detached::send_settings(client, options);
        }
    }

    /// Tells the user about loose files' folders too big to analyse.
    fn refused(&self, folders: &[PathBuf]) {
        for dir in folders {
            let message = detached::refused_message(dir);
            log::info!("{message}");
            self.events.send(LspEvent::ShowMessage {
                server: self.name.clone(),
                typ: lsp_types::MessageType::WARNING,
                message,
            });
        }
    }
}

/// Answers what is still queued for an instance that is not running, so
/// queued requests resolve (their senders drop) and shutdowns are acked.
async fn drain(mut rx: mpsc::UnboundedReceiver<Command>) {
    while let Some(cmd) = rx.recv().await {
        if let Command::Shutdown(ack) = cmd {
            let _ = ack.send(());
        }
    }
}

/// The process spec of `config` at `root`, adjusted by `hook` on a
/// blocking thread.
async fn launch_spec(config: &ServerConfig, root: &Path, hook: Option<LaunchHook>) -> ServerSpec {
    let spec = config.spec();
    let Some(hook) = hook else {
        return spec;
    };
    let fallback = spec.clone();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut spec = spec;
        hook(&mut spec, &root);
        spec
    })
    .await
    .unwrap_or(fallback)
}

async fn run(
    spec: ServerSpec,
    root: PathBuf,
    instance: Instance,
    mut rx: mpsc::UnboundedReceiver<Command>,
    stopped: &AtomicBool,
) {
    // The slot is filled before `Running` is emitted, so a UI reacting to
    // that event can route requests to this server right away.
    let name = spec.name.clone();
    let slot = instance.client.clone();
    let started = LspClient::start_then(spec, root, instance.events.clone(), |client| {
        let _ = slot.set(client.clone());
    });
    let client = match started.await {
        Ok(client) => client,
        Err(e) => {
            log::warn!("[{name}] failed to start: {e}");
            stopped.store(true, Ordering::Relaxed);
            drain(rx).await;
            return;
        }
    };
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Shutdown(ack) => {
                if let Err(e) = client.shutdown().await {
                    log::debug!("[{name}] shutdown: {e}");
                }
                let _ = ack.send(());
                break;
            }
            Command::UpdateDetached => instance.update(&client).await,
            cmd => {
                if let Err(e) = apply(&client, cmd) {
                    log::debug!("[{name}] {e}");
                }
            }
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
        Command::Shutdown(_) | Command::UpdateDetached => Ok(()),
    }
}
