//! [`LspClient`]: a handle to one running language server.

mod dispatch;
mod group;
mod process;
mod resolve;
mod shared;
mod signals;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Exit,
    Initialized, Notification,
};
use lsp_types::request::{Initialize, Request, Shutdown};
use lsp_types::{InitializeResult, InitializedParams, ServerCapabilities, ServerInfo, Uri};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::oneshot;

pub use group::kill_all as kill_all_servers;
pub use process::ServerSpec;
use shared::Shared;
pub use signals::install as kill_servers_on_signals;

use crate::capabilities::initialize_params;
use crate::error::{LspError, Result};
use crate::event::{EventSink, PositionEncoding, ServerStatus};
use crate::sync::{DocumentSync, TextChange};

/// Default limit for ordinary requests.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Limit for the `initialize` request (servers may load a project first).
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(120);
/// Limit for each phase of [`LspClient::shutdown`].
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// What the server told us in its `initialize` response.
#[derive(Debug)]
struct Negotiated {
    capabilities: ServerCapabilities,
    encoding: PositionEncoding,
    server_info: Option<ServerInfo>,
    sync: DocumentSync,
}

/// Terminates the connection and kills the process when the last handle drops.
struct Lifetime {
    shared: Arc<Shared>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
}

impl Lifetime {
    fn kill(&self) {
        let tx = self.kill.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }
}

impl Drop for Lifetime {
    fn drop(&mut self) {
        self.shared.mark_shutting_down();
        self.shared.terminate("client dropped");
        // Dropping `kill` makes the supervisor kill the process.
    }
}

/// A cheap-to-clone handle to one initialized language server.
///
/// Methods that spawn work ([`LspClient::start`], [`LspClient::from_transport`])
/// must run inside a tokio runtime. Request futures can be awaited anywhere;
/// their timeout only applies when polled inside tokio.
#[derive(Clone)]
pub struct LspClient {
    lifetime: Arc<Lifetime>,
    negotiated: Arc<Negotiated>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("name", &self.name())
            .field("root", &self.root())
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl LspClient {
    /// Spawns the server process in `root` and performs `initialize` /
    /// `initialized`. Emits `Starting`, then `Running` (or `Busy` if the
    /// server already reports progress) or `Failed`; stderr lines become
    /// `Log` events.
    pub async fn start(spec: ServerSpec, root: PathBuf, events: EventSink) -> Result<LspClient> {
        Self::start_then(spec, root, events, |_| {}).await
    }

    /// Like [`LspClient::start`], but runs `before_running` with the
    /// initialized client before `Running` is reported, so whoever reacts
    /// to that event can already see the client.
    pub(crate) async fn start_then(
        spec: ServerSpec,
        root: PathBuf,
        events: EventSink,
        before_running: impl FnOnce(&LspClient) + Send,
    ) -> Result<LspClient> {
        let (shared, rx) = Shared::new(
            spec.name.clone(),
            root.clone(),
            events,
            spec.initialization_options.clone(),
            spec.features,
        );
        shared.emit_status(ServerStatus::Starting);
        let spawned = match process::spawn(&spec, &root) {
            Ok(s) => s,
            Err(e) => {
                shared.terminate(e.to_string());
                return Err(e);
            }
        };
        let (kill_tx, kill_rx) = oneshot::channel();
        tokio::spawn(process::supervise(
            shared.clone(),
            spawned.child,
            spawned.group,
            kill_rx,
        ));
        if let Some(stderr) = spawned.stderr {
            tokio::spawn(process::forward_stderr(shared.clone(), stderr));
        }
        tokio::spawn(dispatch::read_loop(
            shared.clone(),
            BufReader::new(spawned.stdout),
        ));
        tokio::spawn(dispatch::write_loop(shared.clone(), rx, spawned.stdin));
        Self::initialize(shared, Some(kill_tx), before_running).await
    }

    /// Connects over an arbitrary byte stream (e.g. `tokio::io::duplex` in
    /// tests) and performs `initialize` / `initialized`.
    pub async fn from_transport<R, W>(
        name: impl Into<String>,
        root: PathBuf,
        reader: R,
        writer: W,
        initialization_options: Option<Value>,
        events: EventSink,
    ) -> Result<LspClient>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (shared, rx) = Shared::new(
            name.into(),
            root,
            events,
            initialization_options,
            crate::Features::default(),
        );
        shared.emit_status(ServerStatus::Starting);
        tokio::spawn(dispatch::read_loop(shared.clone(), BufReader::new(reader)));
        tokio::spawn(dispatch::write_loop(shared.clone(), rx, writer));
        Self::initialize(shared, None, |_| {}).await
    }

    async fn initialize(
        shared: Arc<Shared>,
        kill: Option<oneshot::Sender<()>>,
        before_running: impl FnOnce(&LspClient) + Send,
    ) -> Result<LspClient> {
        let lifetime = Arc::new(Lifetime {
            shared: shared.clone(),
            kill: Mutex::new(kill),
        });
        let params = initialize_params(&shared.root, shared.initialization_options.clone());
        let result = async {
            let params = serde_json::to_value(params)?;
            let value = shared
                .request_value(Initialize::METHOD, params, Some(INITIALIZE_TIMEOUT))
                .await?;
            let result: InitializeResult = serde_json::from_value(value)?;
            let initialized = serde_json::to_value(InitializedParams {})?;
            shared.notify_value(Initialized::METHOD, initialized)?;
            Ok::<_, LspError>(result)
        }
        .await;
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                shared.terminate(format!("initialize failed: {e}"));
                lifetime.kill();
                return Err(e);
            }
        };
        let encoding = PositionEncoding::from_kind(result.capabilities.position_encoding.as_ref());
        let sync = DocumentSync::from_capabilities(&result.capabilities);
        let mut capabilities = result.capabilities;
        // Features turned off for this server look like it lacks them.
        shared.features.mask(&mut capabilities);
        shared.set_encoding(encoding);
        let client = LspClient {
            lifetime,
            negotiated: Arc::new(Negotiated {
                capabilities,
                encoding,
                server_info: result.server_info,
                sync,
            }),
        };
        before_running(&client);
        if !shared.mark_ready() {
            // Died right after answering `initialize`; `Failed` was reported.
            client.lifetime.kill();
            return Err(shared.stopped_error());
        }
        Ok(client)
    }

    fn shared(&self) -> &Arc<Shared> {
        &self.lifetime.shared
    }

    /// Server name from the spec.
    pub fn name(&self) -> &str {
        &self.shared().name
    }

    /// Project root this instance serves.
    pub fn root(&self) -> &Path {
        &self.shared().root
    }

    /// Capabilities from the `initialize` response.
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.negotiated.capabilities
    }

    /// Negotiated position encoding (UTF-16 unless the server chose otherwise).
    pub fn position_encoding(&self) -> PositionEncoding {
        self.negotiated.encoding
    }

    /// `serverInfo` from the `initialize` response.
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.negotiated.server_info.as_ref()
    }

    /// How this server wants documents synchronized.
    pub fn document_sync(&self) -> DocumentSync {
        self.negotiated.sync
    }

    /// False once the server exited, crashed or was shut down.
    pub fn is_alive(&self) -> bool {
        self.shared().is_alive()
    }

    /// Sends a request with [`DEFAULT_REQUEST_TIMEOUT`]. The message is
    /// written immediately; dropping the future sends `$/cancelRequest`.
    pub fn request<R>(
        &self,
        params: R::Params,
    ) -> impl Future<Output = Result<R::Result>> + Send + 'static
    where
        R: Request,
    {
        self.request_with_timeout::<R>(params, Some(DEFAULT_REQUEST_TIMEOUT))
    }

    /// Like [`LspClient::request`] with an explicit timeout (`None`: wait forever).
    pub fn request_with_timeout<R>(
        &self,
        params: R::Params,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<R::Result>> + Send + 'static
    where
        R: Request,
    {
        let pending = serde_json::to_value(params)
            .map_err(LspError::from)
            .map(|v| self.shared().request_value(R::METHOD, v, timeout));
        async move {
            let value = pending?.await?;
            serde_json::from_value(value).map_err(LspError::from)
        }
    }

    /// Sends a notification.
    pub fn notify<N: Notification>(&self, params: N::Params) -> Result<()> {
        self.shared()
            .notify_value(N::METHOD, serde_json::to_value(params)?)
    }

    /// Sends `textDocument/didOpen` if the server wants it.
    pub fn did_open(&self, uri: Uri, language_id: &str, version: i32, text: String) -> Result<()> {
        match self
            .document_sync()
            .did_open(uri, language_id, version, text)
        {
            Some(p) => self.notify::<DidOpenTextDocument>(p),
            None => Ok(()),
        }
    }

    /// Sends `textDocument/didChange` in the form the server wants.
    pub fn did_change(
        &self,
        uri: Uri,
        version: i32,
        changes: Vec<TextChange>,
        full_text: impl FnOnce() -> String,
    ) -> Result<()> {
        match self
            .document_sync()
            .did_change(uri, version, changes, full_text)
        {
            Some(p) => self.notify::<DidChangeTextDocument>(p),
            None => Ok(()),
        }
    }

    /// Sends `textDocument/didSave` if the server wants it.
    pub fn did_save(&self, uri: Uri, text: impl FnOnce() -> String) -> Result<()> {
        match self.document_sync().did_save(uri, text) {
            Some(p) => self.notify::<DidSaveTextDocument>(p),
            None => Ok(()),
        }
    }

    /// Sends `textDocument/didClose` if the server wants it.
    pub fn did_close(&self, uri: Uri) -> Result<()> {
        match self.document_sync().did_close(uri) {
            Some(p) => self.notify::<DidCloseTextDocument>(p),
            None => Ok(()),
        }
    }

    /// Graceful stop: `shutdown` request, `exit` notification, wait for the
    /// connection to close; kills the process if any step times out.
    /// Emits `Stopped`.
    pub async fn shutdown(&self) -> Result<()> {
        let shared = self.shared().clone();
        if !shared.is_alive() {
            return Ok(());
        }
        shared.mark_shutting_down();
        let answer = self
            .request_with_timeout::<Shutdown>((), Some(SHUTDOWN_TIMEOUT))
            .await;
        let _ = self.notify::<Exit>(());
        let closed = tokio::time::timeout(SHUTDOWN_TIMEOUT, shared.terminated()).await;
        if closed.is_err() {
            self.lifetime.kill();
            shared.terminate("killed after shutdown timeout");
        }
        match answer {
            Err(LspError::ServerStopped(_)) | Ok(()) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Kills the server process immediately (no-op for transport clients
    /// besides closing the connection).
    pub fn kill(&self) {
        self.shared().mark_shutting_down();
        self.lifetime.kill();
        self.shared().terminate("killed");
    }
}
