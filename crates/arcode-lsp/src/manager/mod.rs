//! [`LspManager`]: the UI-facing registry of language servers.

mod actor;
mod config;
mod requests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lsp_types::{ServerCapabilities, Uri};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::oneshot;

use actor::{Command, ServerSlot};
pub use config::{ServerConfig, find_root};

use crate::client::LspClient;
use crate::event::{EventReceiver, EventSink, PositionEncoding};
use crate::sync::TextChange;
use crate::uri::path_to_uri;

/// Default limit for [`LspManager::shutdown_all`].
const SHUTDOWN_ALL_TIMEOUT: Duration = Duration::from_secs(5);

struct OpenDoc {
    uri: Uri,
    text: Arc<str>,
    servers: Vec<Arc<ServerSlot>>,
}

#[derive(Default)]
struct State {
    workspace_root: Option<PathBuf>,
    servers: HashMap<(String, PathBuf), Arc<ServerSlot>>,
    documents: HashMap<PathBuf, OpenDoc>,
}

/// Owns a small tokio runtime and every running language server.
///
/// All methods may be called from any thread and never wait for server I/O
/// (except [`LspManager::shutdown_all`], which blocks with a timeout).
/// Request futures are runtime-agnostic and can be awaited on any executor.
pub struct LspManager {
    configs: HashMap<String, ServerConfig>,
    languages: HashMap<String, Vec<String>>,
    events: EventSink,
    handle: Handle,
    state: Mutex<State>,
    runtime: Option<Runtime>,
}

impl std::fmt::Debug for LspManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspManager")
            .field("servers", &self.configs.keys().collect::<Vec<_>>())
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("arcode-lsp")
            .enable_all()
            .build()?;
        let (events, receiver) = EventSink::channel();
        let manager = LspManager {
            configs: servers.into_iter().map(|c| (c.name.clone(), c)).collect(),
            languages,
            events,
            handle: runtime.handle().clone(),
            state: Mutex::new(State::default()),
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
    pub fn open_document(
        &self,
        path: &Path,
        language_id: &str,
        version: i32,
        text: impl Into<Arc<str>>,
    ) {
        let path = absolute(path);
        let uri = path_to_uri(&path);
        let text: Arc<str> = text.into();
        let names = self.languages.get(language_id).cloned().unwrap_or_default();
        let mut state = self.lock();
        if let Some(old) = state.documents.remove(&path) {
            old.servers.iter().for_each(|s| {
                s.send(Command::Close {
                    uri: old.uri.clone(),
                })
            });
        }
        let mut servers = Vec::new();
        for name in names {
            let Some(config) = self.configs.get(&name) else {
                log::warn!("language {language_id} references unknown server {name}");
                continue;
            };
            let root = find_root(&path, &config.root_markers, state.workspace_root.as_deref());
            let slot = state
                .servers
                .entry((name, root.clone()))
                .or_insert_with(|| actor::spawn(&self.handle, config, root, self.events.clone()))
                .clone();
            slot.send(Command::Open {
                uri: uri.clone(),
                language_id: language_id.to_string(),
                version,
                text: text.clone(),
            });
            servers.push(slot);
        }
        state.documents.insert(path, OpenDoc { uri, text, servers });
    }

    /// Reports edits. Ranges are UTF-16 (the LSP default); servers that
    /// negotiated another encoding receive `full_text` instead.
    pub fn change_document(
        &self,
        path: &Path,
        version: i32,
        changes: Vec<TextChange>,
        full_text: impl Into<Arc<str>>,
    ) {
        self.change_document_encoded(path, version, changes, PositionEncoding::Utf16, full_text);
    }

    /// Reports edits whose ranges use `encoding`. Servers using that encoding
    /// get incremental changes if they want them; others get `full_text`.
    pub fn change_document_encoded(
        &self,
        path: &Path,
        version: i32,
        changes: Vec<TextChange>,
        encoding: PositionEncoding,
        full_text: impl Into<Arc<str>>,
    ) {
        let full_text: Arc<str> = full_text.into();
        let mut state = self.lock();
        let Some(doc) = state.documents.get_mut(&absolute(path)) else {
            return;
        };
        doc.text = full_text.clone();
        for slot in &doc.servers {
            slot.send(Command::Change {
                uri: doc.uri.clone(),
                version,
                changes: changes.clone(),
                encoding,
                full_text: full_text.clone(),
            });
        }
    }

    /// Sends `didSave` (with the latest text if a server asked for it).
    pub fn save_document(&self, path: &Path) {
        let state = self.lock();
        let Some(doc) = state.documents.get(&absolute(path)) else {
            return;
        };
        for slot in &doc.servers {
            slot.send(Command::Save {
                uri: doc.uri.clone(),
                text: doc.text.clone(),
            });
        }
    }

    /// Sends `didClose` and forgets the document.
    pub fn close_document(&self, path: &Path) {
        let Some(doc) = self.lock().documents.remove(&absolute(path)) else {
            return;
        };
        for slot in &doc.servers {
            slot.send(Command::Close {
                uri: doc.uri.clone(),
            });
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
