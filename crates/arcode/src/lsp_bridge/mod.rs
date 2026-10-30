//! Connects editors to language servers: opens/changes/saves/closes
//! documents through an [`LspManager`], collects diagnostics in a
//! [`DiagnosticStore`], pushes them into editors and tracks server status
//! for the status bar.
//!
//! The manager (and with it every server) is created lazily, the first time
//! a file whose language has servers is opened.

pub mod convert;
mod events;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arcode_editor::{Editor, EditorEvent};
use arcode_lsp::lsp_types::Position;
use arcode_lsp::{DiagnosticCounts, DiagnosticStore, EventReceiver, LspManager, PositionEncoding};
use arcode_ui_kit::settings;
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, SharedString, Subscription, Task, WeakEntity,
};

use convert::ServerState;

/// How long quitting waits for servers to shut down gracefully.
const QUIT_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1500);
/// Upper bound on events handled in one UI update.
const EVENT_BATCH: usize = 256;

/// Events for the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LspBridgeEvent {
    /// The diagnostic store changed.
    DiagnosticsChanged,
    /// A server's status or progress changed.
    StatusChanged,
    /// Something the user should see (server failure, `showMessage`).
    Message(SharedString),
}

/// An editor known to the bridge.
struct Doc {
    editor: WeakEntity<Editor>,
    path: PathBuf,
    /// Whether `didOpen` was sent (the language has servers).
    synced: bool,
    _subscriptions: [Subscription; 2],
}

/// The LSP side of a workspace window.
pub struct LspBridge {
    enabled: bool,
    manager: Option<Arc<LspManager>>,
    root: Option<PathBuf>,
    store: DiagnosticStore,
    servers: HashMap<String, ServerState>,
    docs: HashMap<EntityId, Doc>,
    _events: Option<Task<()>>,
    _quit: Subscription,
}

impl EventEmitter<LspBridgeEvent> for LspBridge {}

impl LspBridge {
    /// Creates the bridge; no server runs until a document needs one.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let quit = cx.on_app_quit(|bridge: &mut Self, _| {
            if let Some(manager) = bridge.manager.take() {
                manager.shutdown_all_with_timeout(QUIT_SHUTDOWN_TIMEOUT);
            }
            async {}
        });
        cx.on_release(|bridge: &mut Self, _| bridge.stop_servers())
            .detach();
        Self {
            enabled: true,
            manager: None,
            root: None,
            store: DiagnosticStore::new(),
            servers: HashMap::new(),
            docs: HashMap::new(),
            _events: None,
            _quit: quit,
        }
    }

    /// Enables or disables starting language servers (tests turn it off).
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// The merged diagnostics of every server.
    pub fn store(&self) -> &DiagnosticStore {
        &self.store
    }

    /// Project-wide diagnostic totals.
    pub fn counts(&self) -> DiagnosticCounts {
        self.store.counts()
    }

    /// Sets the project root. A different root stops all servers (they
    /// restart lazily for the new project) and clears diagnostics.
    pub fn set_project_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.stop_servers();
        self.root = root;
        self.store = DiagnosticStore::new();
        self.servers.clear();
        cx.emit(LspBridgeEvent::DiagnosticsChanged);
        cx.emit(LspBridgeEvent::StatusChanged);
        cx.notify();
    }

    /// Stops every server in the background (graceful, then killed).
    fn stop_servers(&mut self) {
        self._events = None;
        if let Some(manager) = self.manager.take() {
            let spawned = std::thread::Builder::new()
                .name("arcode-lsp-shutdown".into())
                .spawn(move || manager.shutdown_all());
            if let Err(err) = spawned {
                log::warn!("cannot spawn the LSP shutdown thread: {err}");
            }
        }
    }

    /// Status text for the first server of `language_id` that has started,
    /// e.g. `rust-analyzer: ready`.
    pub fn status_for_language(&self, language_id: &str, cx: &App) -> Option<SharedString> {
        let language = settings(cx).language(language_id)?;
        language.servers.iter().find_map(|name| {
            self.servers
                .get(name)
                .map(|state| convert::status_text(name, state).into())
        })
    }

    /// Position encoding `server` uses for `path` (UTF-16 if unknown).
    pub fn encoding(&self, path: &Path, server: &str) -> PositionEncoding {
        self.manager
            .as_ref()
            .and_then(|m| m.position_encoding(path, server))
            .unwrap_or_default()
    }

    /// Converts a server position in `path` to a point in `editor`.
    pub fn point_in(
        &self,
        editor: &Editor,
        server: &str,
        position: Position,
    ) -> arcode_buffer::Point {
        let encoding = editor
            .path()
            .map(|p| self.encoding(p, server))
            .unwrap_or_default();
        convert::to_point(editor.buffer(), position, encoding)
    }

    /// Starts tracking `editor`: sends `didOpen` when its language has
    /// servers, forwards edits and saves, applies known diagnostics and
    /// sends `didClose` when the editor is dropped.
    pub fn register_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let (path, language_id, version, has_servers) = {
            let e = editor.read(cx);
            let Some(path) = e.path().map(Path::to_path_buf) else {
                return;
            };
            let id = e.language().id.to_lowercase();
            let has_servers = settings(cx)
                .language(&id)
                .is_some_and(|l| !l.servers.is_empty());
            (path, id, e.buffer().version(), has_servers)
        };
        let synced = has_servers && self.enabled;
        if synced && let Some(manager) = self.ensure_manager(cx) {
            // A rope snapshot is cheap; the full text is only built while
            // `didOpen` is sent, so no long-lived copy of a large file stays.
            let rope = editor.read(cx).buffer().text().clone();
            manager.open_document_lazy(&path, &language_id, lsp_version(version), move || {
                rope.to_string()
            });
        }
        let events = cx.subscribe(editor, |bridge, editor, event: &EditorEvent, cx| {
            bridge.on_editor_event(&editor, event, cx)
        });
        let id = editor.entity_id();
        let release = cx.observe_release(editor, move |bridge, _, _| bridge.forget(id));
        self.docs.insert(
            id,
            Doc {
                editor: editor.downgrade(),
                path: path.clone(),
                synced,
                _subscriptions: [events, release],
            },
        );
        self.push_diagnostics(&path, cx);
    }

    fn forget(&mut self, id: EntityId) {
        let Some(doc) = self.docs.remove(&id) else {
            return;
        };
        if doc.synced
            && let Some(manager) = &self.manager
        {
            manager.close_document(&doc.path);
        }
    }

    fn on_editor_event(
        &mut self,
        editor: &Entity<Editor>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.docs.get(&editor.entity_id()) else {
            return;
        };
        if !doc.synced {
            return;
        }
        let Some(manager) = self.manager.clone() else {
            return;
        };
        match event {
            EditorEvent::Edited { edits, version } => {
                let encoding = manager
                    .running_servers(&doc.path)
                    .first()
                    .and_then(|server| manager.position_encoding(&doc.path, server))
                    .unwrap_or_default();
                let changes = convert::text_changes(edits, encoding);
                let rope = editor.read(cx).buffer().text().clone();
                manager.change_document_lazy(
                    &doc.path,
                    lsp_version(*version),
                    changes,
                    encoding,
                    move || rope.to_string(),
                );
            }
            EditorEvent::Saved => manager.save_document(&doc.path),
            EditorEvent::SelectionsChanged
            | EditorEvent::DirtyChanged(_)
            | EditorEvent::GutterClicked { .. }
            | EditorEvent::CodeLensClicked { .. } => {}
        }
    }

    /// The manager, created (with its event loop) on first use.
    fn ensure_manager(&mut self, cx: &mut Context<Self>) -> Option<Arc<LspManager>> {
        if let Some(manager) = &self.manager {
            return Some(manager.clone());
        }
        let (servers, languages) = convert::server_configs(settings(cx));
        let (manager, events) = match LspManager::try_new(servers, languages) {
            Ok(created) => created,
            Err(err) => {
                log::error!("cannot start the LSP runtime: {err}");
                cx.emit(LspBridgeEvent::Message(
                    format!("Language servers unavailable: {err}").into(),
                ));
                return None;
            }
        };
        manager.set_workspace_root(self.root.clone());
        let manager = Arc::new(manager);
        self.manager = Some(manager.clone());
        self._events = Some(Self::poll_events(events, cx));
        Some(manager)
    }

    fn poll_events(events: EventReceiver, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                while batch.len() < EVENT_BATCH {
                    match events.try_recv() {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                let alive = this.update(cx, |bridge, cx| {
                    for event in batch {
                        bridge.handle_event(event, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
    }

    /// Sends the merged diagnostics for `path` to its open editors.
    pub(super) fn push_diagnostics(&self, path: &Path, cx: &mut App) {
        let editors: Vec<Entity<Editor>> = self
            .docs
            .values()
            .filter(|d| d.path == path)
            .filter_map(|d| d.editor.upgrade())
            .collect();
        for editor in editors {
            let diagnostics: Vec<_> = {
                let e = editor.read(cx);
                self.store
                    .for_path(path)
                    .into_iter()
                    .map(|(server, d)| {
                        let encoding = self.encoding(path, server);
                        convert::editor_diagnostic(e.buffer(), server, d, encoding)
                    })
                    .collect()
            };
            editor.update(cx, |e, cx| e.set_diagnostics(diagnostics, cx));
        }
    }
}

/// LSP document versions are `i32`.
fn lsp_version(version: u64) -> i32 {
    i32::try_from(version).unwrap_or(i32::MAX)
}
