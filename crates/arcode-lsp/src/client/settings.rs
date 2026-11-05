//! Changing the settings of a running server.

use std::future::Future;
use std::time::Duration;

use lsp_types::DidChangeConfigurationParams;
use lsp_types::notification::DidChangeConfiguration;
use serde_json::Value;

use super::LspClient;

/// How long [`LspClient::change_settings`] waits for the server to ask for
/// the new settings.
const PULL_WAIT: Duration = Duration::from_secs(10);

impl LspClient {
    /// Replaces the settings this client gives the server (the
    /// initialization options, which also answer `workspace/configuration`)
    /// and sends `workspace/didChangeConfiguration` with them.
    ///
    /// Servers that pull their settings ignore the notification's payload
    /// and ask with `workspace/configuration` instead. The future resolves
    /// to true once such a request was answered: a message sent after that
    /// reaches the server after the new settings. False when the server did
    /// not ask within 10 seconds or the connection ended. Await it inside a
    /// tokio runtime.
    pub fn change_settings(&self, settings: Value) -> impl Future<Output = bool> + Send + 'static {
        let shared = self.shared().clone();
        let mut answers = shared.settings_answers();
        answers.borrow_and_update();
        shared.set_options(Some(settings.clone()));
        let sent = self.notify::<DidChangeConfiguration>(DidChangeConfigurationParams { settings });
        async move {
            sent.is_ok()
                && matches!(
                    tokio::time::timeout(PULL_WAIT, answers.changed()).await,
                    Ok(Ok(()))
                )
        }
    }
}
