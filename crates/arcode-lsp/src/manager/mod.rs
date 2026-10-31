//! [`LspManager`]: the UI-facing registry of language servers.

mod actor;
mod answer;
mod changes;
mod config;
mod lifecycle;
mod requests;
mod text;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lsp_types::{ServerCapabilities, Uri};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::oneshot;

use actor::{Command, MakeOpenText, ServerSlot};
pub use answer::Answer;
pub use config::{ServerConfig, find_root};
use text::LazyText;

use crate::client::LspClient;
use crate::event::{EventReceiver, EventSink, PositionEncoding};
use crate::uri::path_to_uri;

/// Default limit for [`LspManager::shutdown_all`].
const SHUTDOWN_ALL_TIMEOUT: Duration = Duration::from_secs(5);

struct OpenDoc {
    uri: Uri,
    language_id: String,
    /// Latest version, for a server (re)started while the document is open.
    version: i32,
    /// Latest full text, for `didSave` with `includeText`.
    text: Arc<LazyText>,
    /// Builds the latest text for a `didOpen`.
    open_text: Arc<MakeOpenText>,
    /// Its servers, in priority order.
    servers: Vec<Arc<ServerSlot>>,
}

#[derive(Default)]
struct State {
    configs: HashMap<String, ServerConfig>,
    /// Language id -> server names in priority order.
    languages: HashMap<String, Vec<String>>,
    workspace_root: Option<PathBuf>,
    servers: HashMap<(String, PathBuf), Arc<ServerSlot>>,
    documents: HashMap<PathBuf, OpenDoc>,
    /// Files outside the workspace that join its server when opened; see
    /// [`LspManager::attach_to_workspace`].
    attached: HashSet<PathBuf>,
    /// Instances being restarted: the replacement starts once the old one
    /// acknowledged its shutdown, so their events never interleave.
    stopping: HashMap<(String, PathBuf), oneshot::Receiver<()>>,
}

/// Owns a small tokio runtime and every running language server.
///
/// All methods may be called from any thread and never wait for server I/O
/// (except [`LspManager::shutdown_all`], which blocks with a timeout).
/// Request futures are runtime-agnostic and can be awaited on any executor.
///
/// On Linux with glibc, creating a manager pins the allocator's mmap
/// threshold to 1 MiB so the transient copies of large documents are
/// returned to the OS (see `memory.rs`).
pub struct LspManager {
    events: EventSink,
    handle: Handle,
    state: Mutex<State>,
    runtime: Option<Runtime>,
}

impl std::fmt::Debug for LspManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self.lock().configs.keys().cloned().collect();
        f.debug_struct("LspManager")
            .field("servers", &names)
            .finish_non_exhaustive()
    }
}

impl LspManager {
    /// Creates the manager. `languages` maps a language id to server names
    /// in priority order.
    ///
    /// # Panics
    /// If the OS refuses to create the runtime's threads; see
    /// [`LspManager::try_new`].
    pub fn new(
        servers: Vec<ServerConfig>,
        languages: HashMap<String, Vec<String>>,
    ) -> (LspManager, EventReceiver) {
        Self::try_new(servers, languages).expect("failed to create the arcode-lsp runtime")
    }

    /// Like [`LspManager::new`] but reports runtime creation errors.
    pub fn try_new(
        servers: Vec<ServerConfig>,
        languages: HashMap<String, Vec<String>>,
    ) -> std::io::Result<(LspManager, EventReceiver)> {
        crate::memory::tune_allocator();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("arcode-lsp")
            .enable_all()
            .build()?;
        let (events, receiver) = EventSink::channel();
        let state = State {
            configs: servers.into_iter().map(|c| (c.name.clone(), c)).collect(),
            languages: config::dedupe_languages(languages),
            ..State::default()
        };
        let manager = LspManager {
            events,
            handle: runtime.handle().clone(),
            state: Mutex::new(state),
            runtime: Some(runtime),
        };
        Ok((manager, receiver))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sets the fallback root for files without root markers inside it.
    pub fn set_workspace_root(&self, root: Option<PathBuf>) {
        self.lock().workspace_root = root;
    }

    /// Opens a document, starting (once) each server configured for
    /// `language_id` for the file's project root. Re-opening an open path
    /// closes it first. Messages are queued until servers are initialized.
    ///
    /// A file marked with [`LspManager::attach_to_workspace`] (a standard
    /// library source a definition led to) joins a server already running
    /// for the workspace instead of starting one for its own root.
    pub fn open_document(
        &self,
        path: &Path,
        language_id: &str,
        version: i32,
        text: impl Into<Arc<str>>,
    ) {
        let text: Arc<str> = text.into();
        let for_open = text.clone();
        let make: Arc<MakeOpenText> = Arc::new(move || for_open.to_string());
        self.open_with(path, language_id, version, make, LazyText::ready(text));
    }

    /// Like [`LspManager::open_document`], but the text is produced by
    /// `text` only when it is needed: once per server when its `didOpen`
    /// is sent (and dropped right after), and again for a `didSave` that
    /// includes the text. Nothing keeps a full copy of the document, which
    /// matters for very large files (pass a cheap rope snapshot).
    pub fn open_document_lazy(
        &self,
        path: &Path,
        language_id: &str,
        version: i32,
        text: impl Fn() -> String + Send + Sync + 'static,
    ) {
        let make: Arc<MakeOpenText> = Arc::new(text);
        let for_save = make.clone();
        let saved = LazyText::deferred(move || for_save());
        self.open_with(path, language_id, version, make, saved);
    }

    fn open_with(
        &self,
        path: &Path,
        language_id: &str,
        version: i32,
        make: Arc<MakeOpenText>,
        text: Arc<LazyText>,
    ) {
        let path = absolute(path);
        let uri = path_to_uri(&path);
        let mut state = self.lock();
        if let Some(old) = state.documents.remove(&path) {
            old.servers.iter().for_each(|s| {
                s.send(Command::Close {
                    uri: old.uri.clone(),
                })
            });
        }
        let doc = OpenDoc {
            uri,
            language_id: language_id.to_string(),
            version,
            text,
            open_text: make,
            servers: Vec::new(),
        };
        state.documents.insert(path.clone(), doc);
        self.attach_servers(&mut state, &path);
    }

    /// Makes `path`'s servers match its language's configured list:
    /// starts (once per root) and opens the document on servers it lacks,
    /// closes it on servers no longer listed, keeping priority order.
    fn attach_servers(&self, state: &mut State, path: &Path) {
        let Some(doc) = state.documents.get(path) else {
            return;
        };
        let (uri, language_id, version) = (doc.uri.clone(), doc.language_id.clone(), doc.version);
        let (open_text, current) = (doc.open_text.clone(), doc.servers.clone());
        let names = state
            .languages
            .get(&language_id)
            .cloned()
            .unwrap_or_default();
        let mut servers: Vec<Arc<ServerSlot>> = Vec::new();
        for name in names {
            if servers.iter().any(|s| s.name == name) {
                continue; // listed twice: serve the document once
            }
            let Some(config) = state.configs.get(&name).cloned() else {
                log::warn!("language {language_id} references unknown server {name}");
                continue;
            };
            let mut root = find_root(path, &config.root_markers, state.workspace_root.as_deref());
            if let Some(shared) = config::shared_root(state, &name, path, &root) {
                root = shared;
            }
            if let Some(slot) = current.iter().find(|s| s.name == name && s.root == root) {
                servers.push(slot.clone());
                continue;
            }
            let key = (name, root.clone());
            let after = state.stopping.remove(&key);
            let slot = state
                .servers
                .entry(key)
                .or_insert_with(|| {
                    actor::spawn(&self.handle, &config, root, self.events.clone(), after)
                })
                .clone();
            slot.send(Command::Open {
                uri: uri.clone(),
                language_id: language_id.clone(),
                version,
                text: open_text.clone(),
            });
            servers.push(slot);
        }
        for old in &current {
            if !servers.iter().any(|s| Arc::ptr_eq(s, old)) {
                old.send(Command::Close { uri: uri.clone() });
            }
        }
        if let Some(doc) = state.documents.get_mut(path) {
            doc.servers = servers;
        }
    }

    /// Running servers for an open document, in priority order.
    fn running(&self, path: &Path) -> Vec<(Arc<ServerSlot>, LspClient)> {
        let state = self.lock();
        let Some(doc) = state.documents.get(&absolute(path)) else {
            return Vec::new();
        };
        doc.servers
            .iter()
            .filter_map(|s| s.running_client().map(|c| (s.clone(), c.clone())))
            .collect()
    }

    /// Every started server instance as `(name, root)`, sorted.
    pub fn server_instances(&self) -> Vec<(String, PathBuf)> {
        let mut out: Vec<_> = self
            .lock()
            .servers
            .values()
            .map(|s| (s.name.clone(), s.root.clone()))
            .collect();
        out.sort();
        out
    }

    /// Names of the servers currently running for `path`, in priority order.
    pub fn running_servers(&self, path: &Path) -> Vec<String> {
        self.running(path)
            .into_iter()
            .map(|(s, _)| s.name.clone())
            .collect()
    }

    /// Position encoding negotiated by `server` for `path`'s project.
    pub fn position_encoding(&self, path: &Path, server: &str) -> Option<PositionEncoding> {
        self.running(path)
            .into_iter()
            .find(|(s, _)| s.name == server)
            .map(|(_, c)| c.position_encoding())
    }

    /// Capabilities of `server` for `path`'s project.
    pub fn capabilities(&self, path: &Path, server: &str) -> Option<ServerCapabilities> {
        self.running(path)
            .into_iter()
            .find(|(s, _)| s.name == server)
            .map(|(_, c)| c.capabilities().clone())
    }

    /// Gracefully stops every server, blocking up to 5 seconds. Call from a
    /// thread that is not one of this manager's runtime workers.
    pub fn shutdown_all(&self) {
        self.shutdown_all_with_timeout(SHUTDOWN_ALL_TIMEOUT);
    }

    /// Like [`LspManager::shutdown_all`] with a custom limit. Servers still
    /// running afterwards are killed when the manager is dropped.
    pub fn shutdown_all_with_timeout(&self, timeout: Duration) {
        let slots: Vec<Arc<ServerSlot>> = {
            let mut state = self.lock();
            state.documents.clear();
            state.servers.drain().map(|(_, s)| s).collect()
        };
        if slots.is_empty() {
            return;
        }
        let acks: Vec<_> = slots
            .iter()
            .map(|slot| {
                let (tx, rx) = oneshot::channel();
                slot.send(Command::Shutdown(tx));
                rx
            })
            .collect();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.handle.spawn(async move {
            for ack in acks {
                let _ = ack.await;
            }
            let _ = done_tx.send(());
        });
        let _ = done_rx.recv_timeout(timeout);
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            // Dropping tasks drops the children, which are `kill_on_drop`.
            runtime.shutdown_background();
        }
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
