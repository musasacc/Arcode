//! Handling of events coming from language servers.

use std::path::Path;

use arcode_editor::Editor;
use arcode_lsp::lsp_types::MessageType;
use arcode_lsp::{EventReceiver, LspEvent, ServerStatus};
use arcode_ui_kit::settings;
use gpui::{App, Context, Entity, Task};

use super::convert::{self, Progress, ServerState};
use super::edits::EditSource;
use super::{EVENT_BATCH, LspBridge, LspBridgeEvent};

impl LspBridge {
    pub(super) fn poll_events(events: EventReceiver, cx: &mut Context<Self>) -> Task<()> {
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
                    bridge.flush_logs(cx);
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
                // Only the servers of the editor's language: a file switched
                // to another language (or Plain Text) by hand shows no
                // squiggles from a server that still compiles it.
                let language = settings(cx).language(&e.language().id.to_lowercase());
                let serves =
                    |server: &str| language.is_some_and(|l| l.servers.iter().any(|s| s == server));
                self.store
                    .for_path(path)
                    .into_iter()
                    .filter(|(server, _)| serves(server))
                    .map(|(server, d)| {
                        let encoding = self.encoding(path, server);
                        convert::editor_diagnostic(e.buffer(), server, d, encoding)
                    })
                    .collect()
            };
            editor.update(cx, |e, cx| e.set_diagnostics(diagnostics, cx));
        }
    }

    /// Applies one server event.
    pub(crate) fn handle_event(&mut self, event: LspEvent, cx: &mut Context<Self>) {
        match event {
            LspEvent::Diagnostics {
                server,
                path,
                diagnostics,
                ..
            } => {
                self.store.update(&server, path.clone(), diagnostics);
                self.push_diagnostics(&path, cx);
                cx.emit(LspBridgeEvent::DiagnosticsChanged);
                cx.notify();
            }
            LspEvent::ServerStatus {
                server,
                status,
                root,
            } => {
                log::info!("{server}: {status:?}");
                let server_name = server.clone();
                self.log_line(&server, format!("--- {status:?} ({})", root.display()));
                if let ServerStatus::Failed(reason) = &status {
                    self.report_failure(&server, reason, cx);
                }
                if matches!(status, ServerStatus::Failed(_) | ServerStatus::Stopped) {
                    // A crashed or stopped instance's diagnostics would go
                    // stale as the user edits.
                    self.drop_instance_diagnostics(&server, &root, cx);
                }
                let state = self.servers.entry(server).or_default();
                if !status.is_running() {
                    state.progress.clear();
                }
                // Hints asked while the server started or indexed may be
                // missing; ask again whenever it becomes idle.
                let idle = status == ServerStatus::Running && state.status != status;
                state.status = status;
                if idle {
                    self.refresh_inlay_hints(Some(&server_name), cx);
                }
                cx.emit(LspBridgeEvent::StatusChanged);
                cx.notify();
            }
            LspEvent::Progress {
                server,
                token,
                title,
                message,
                percentage,
                done,
            } => {
                let state = self.servers.entry(server).or_insert_with(|| ServerState {
                    status: ServerStatus::Running,
                    progress: Vec::new(),
                });
                state.progress.retain(|(t, _)| *t != token);
                if !done {
                    let progress = Progress {
                        title,
                        message,
                        percentage,
                    };
                    state.progress.push((token, progress));
                }
                cx.emit(LspBridgeEvent::StatusChanged);
                cx.notify();
            }
            LspEvent::ShowMessage {
                server,
                typ,
                message,
            } => {
                self.log_line(&server, format!("[{}] {message}", level_name(typ)));
                if typ == MessageType::ERROR || typ == MessageType::WARNING {
                    let text = format!("{server}: {}", convert::first_line(&message));
                    cx.emit(LspBridgeEvent::Message(text.into()));
                } else {
                    log::info!("{server}: {message}");
                }
            }
            LspEvent::Log {
                server,
                level,
                message,
            } => {
                log::debug!("{server}: {message}");
                self.log_line(&server, format!("[{}] {message}", level_name(level)));
            }
            LspEvent::ApplyEdit {
                server,
                encoding,
                label,
                edit,
                reply,
                ..
            } => {
                log::info!(
                    "{server}: workspace/applyEdit {}",
                    label.as_deref().unwrap_or("")
                );
                self.apply_workspace_edit(edit, encoding, label, reply, EditSource::Server, cx);
            }
            LspEvent::Refresh {
                server, feature, ..
            } => {
                if feature == "inlayHint" {
                    self.refresh_inlay_hints(Some(&server), cx);
                } else {
                    log::debug!("{server}: {feature} refresh ignored");
                }
            }
        }
    }
}

/// A short name for a message severity, for the server log.
fn level_name(level: MessageType) -> &'static str {
    match level {
        MessageType::ERROR => "error",
        MessageType::WARNING => "warn",
        MessageType::INFO => "info",
        _ => "log",
    }
}
