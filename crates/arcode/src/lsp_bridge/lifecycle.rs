//! Creating the language server manager on first use, and stopping it.

use std::sync::Arc;

use arcode_lsp::LspManager;
use arcode_ui_kit::settings;
use gpui::Context;

use super::{LspBridge, LspBridgeEvent, bundled, convert};

impl LspBridge {
    /// Stops every server in the background (graceful, then killed).
    pub(super) fn stop_servers(&mut self) {
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

    /// The manager, created (with its event loop) on first use.
    pub(super) fn ensure_manager(&mut self, cx: &mut Context<Self>) -> Option<Arc<LspManager>> {
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
        manager.set_low_end_mode(settings(cx).ui.low_end_mode);
        // Bundled servers are looked up as each one starts, off this thread.
        manager.set_launch_hook(bundled::launch_hook(self.runtime_notes.clone()));
        let manager = Arc::new(manager);
        self.manager = Some(manager.clone());
        self._events = Some(Self::poll_events(events, cx));
        Some(manager)
    }
}
