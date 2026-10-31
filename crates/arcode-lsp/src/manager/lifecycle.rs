//! Changing the server set while documents are open: live configuration
//! reloads and restarting one server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::oneshot;

use super::actor::{Command, ServerSlot};
use super::{LspManager, ServerConfig, State};

impl LspManager {
    /// Applies a new configuration without touching servers it leaves as
    /// they were. Servers whose [`ServerConfig`] changed or that are gone
    /// are shut down; open documents then get exactly their language's
    /// servers (new or changed ones are started and sent `didOpen` with the
    /// latest text, removed ones get `didClose`). Returns the names of the
    /// servers that were stopped, sorted.
    pub fn reconfigure(
        &self,
        servers: Vec<ServerConfig>,
        languages: HashMap<String, Vec<String>>,
    ) -> Vec<String> {
        let mut state = self.lock();
        let configs: HashMap<String, ServerConfig> =
            servers.into_iter().map(|c| (c.name.clone(), c)).collect();
        let mut changed: Vec<String> = state
            .configs
            .iter()
            .filter(|(name, old)| configs.get(*name) != Some(*old))
            .map(|(name, _)| name.clone())
            .collect();
        changed.sort();
        state.configs = configs;
        state.languages = super::config::dedupe_languages(languages);
        let stopped =
            self.stop_instances(&mut state, |(name, _)| changed.iter().any(|c| c == name));
        self.reattach_all(&mut state);
        stopped
    }

    /// Stops every instance of `server` and starts it again for the open
    /// documents that use it (each gets `didOpen` with its latest text).
    /// Returns false when no instance was running.
    pub fn restart_server(&self, server: &str) -> bool {
        let mut state = self.lock();
        let stopped = self.stop_instances(&mut state, |(name, _)| name == server);
        self.reattach_all(&mut state);
        !stopped.is_empty()
    }

    /// Shuts down the instances no open document uses any more (the last
    /// file of their language or project was closed). Returns the distinct
    /// names stopped, sorted. Opening such a file later starts its server
    /// again (after the old instance finished shutting down).
    pub fn stop_idle_servers(&self) -> Vec<String> {
        let mut state = self.lock();
        let used: Vec<Arc<ServerSlot>> = state
            .documents
            .values()
            .flat_map(|d| d.servers.iter().cloned())
            .collect();
        let idle: Vec<(String, PathBuf)> = state
            .servers
            .iter()
            .filter(|(_, slot)| !used.iter().any(|u| Arc::ptr_eq(u, slot)))
            .map(|(key, _)| key.clone())
            .collect();
        self.stop_instances(&mut state, |key| idle.contains(key))
    }

    /// Whether an instance of `server` is starting or running (not
    /// crashed, failed or stopped).
    pub fn is_server_running(&self, server: &str) -> bool {
        self.lock()
            .servers
            .values()
            .any(|s| s.name == server && s.is_usable())
    }

    /// The configuration of `server`, if it is known.
    pub fn server_config(&self, server: &str) -> Option<ServerConfig> {
        self.lock().configs.get(server).cloned()
    }

    /// Names of the servers configured for `language_id`, in priority order.
    pub fn servers_for_language(&self, language_id: &str) -> Vec<String> {
        self.lock()
            .languages
            .get(language_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Shuts down the instances whose `(name, root)` passes `stop`, removes them from
    /// every document and remembers their shutdown so a replacement waits
    /// for it. Returns the distinct names stopped, sorted.
    fn stop_instances(
        &self,
        state: &mut State,
        stop: impl Fn(&(String, PathBuf)) -> bool,
    ) -> Vec<String> {
        let keys: Vec<(String, PathBuf)> = state
            .servers
            .keys()
            .filter(|key| stop(key))
            .cloned()
            .collect();
        let mut names = Vec::new();
        let mut slots: Vec<Arc<ServerSlot>> = Vec::new();
        for key in keys {
            let Some(slot) = state.servers.remove(&key) else {
                continue;
            };
            let (tx, rx) = oneshot::channel();
            slot.send(Command::Shutdown(tx));
            state.stopping.insert(key.clone(), rx);
            if !names.contains(&key.0) {
                names.push(key.0);
            }
            slots.push(slot);
        }
        for doc in state.documents.values_mut() {
            doc.servers
                .retain(|s| !slots.iter().any(|stopped| Arc::ptr_eq(s, stopped)));
        }
        names.sort();
        names
    }

    /// Re-syncs every open document with its configured servers; stops
    /// that no replacement claimed are forgotten. Workspace documents go
    /// first so the files attached to their instance (library sources, see
    /// [`LspManager::attach_to_workspace`]) find its replacement running.
    fn reattach_all(&self, state: &mut State) {
        let mut paths: Vec<PathBuf> = state.documents.keys().cloned().collect();
        paths.sort_by_key(|p| state.attached.contains(p));
        for path in paths {
            self.attach_servers(state, &path);
        }
        state.stopping.clear();
    }
}
