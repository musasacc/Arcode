//! Handling of events coming from language servers.

use arcode_lsp::lsp_types::MessageType;
use arcode_lsp::{LspEvent, ServerStatus};
use gpui::Context;

use super::convert::{self, Progress, ServerState};
use super::{LspBridge, LspBridgeEvent};

impl LspBridge {
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
            LspEvent::ServerStatus { server, status, .. } => {
                log::info!("{server}: {status:?}");
                if let ServerStatus::Failed(reason) = &status {
                    let reason = convert::first_line(reason);
                    cx.emit(LspBridgeEvent::Message(
                        format!("{server} failed: {reason}").into(),
                    ));
                }
                let state = self.servers.entry(server).or_default();
                if !status.is_running() {
                    state.progress.clear();
                }
                state.status = status;
                cx.emit(LspBridgeEvent::StatusChanged);
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
            }
            LspEvent::ShowMessage {
                server,
                typ,
                message,
            } => {
                if typ == MessageType::ERROR || typ == MessageType::WARNING {
                    let text = format!("{server}: {}", convert::first_line(&message));
                    cx.emit(LspBridgeEvent::Message(text.into()));
                } else {
                    log::info!("{server}: {message}");
                }
            }
            LspEvent::Log {
                server, message, ..
            } => log::debug!("{server}: {message}"),
            LspEvent::ApplyEdit { server, .. } => {
                log::info!("{server}: workspace/applyEdit is not supported yet")
            }
        }
    }
}
