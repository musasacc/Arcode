//! Enabling, disabling and installing plugins (§3 "State", "Installed").
//!
//! - Bundled plugins are enabled unless the user disabled them.
//! - Installed (third-party) plugins start **disabled**. Enabling one
//!   approves its current `plugin.wasm`: the sha256 goes into
//!   `plugins.toml` and every activation checks it, so a file swapped
//!   behind the user's back does not run ([`PluginOutput::NeedsApproval`]).
//! - An update that asks for different capabilities is disabled until the
//!   user enables it again; one with the same capabilities stays enabled.
//! - Dev ("Load Unpacked") plugins are enabled; disabling one lasts for the
//!   session.
//! - Only installed plugins can be uninstalled: the folder the install
//!   created is removed with its `plugins.toml` entry and its grants in
//!   every project.
//! - Every window has its own manager: [`PluginManager::sync_from_disk`]
//!   catches up with what another window or process changed.
//!
//! [`PluginOutput::NeedsApproval`]: crate::output::PluginOutput::NeedsApproval

use std::path::Path;

use super::PluginManager;
use crate::cache::wasm_sha256;
use crate::cancel::lock;
use crate::grant_files::forget_everywhere;
use crate::install::{InstallError, InstallReport, install_from_folder, precompile};
use crate::manifest::{PluginId, PluginSource};
use crate::plugin_state::PluginState;
use crate::registry::{RegisteredPlugin, Registry};

impl PluginManager {
    /// Enables or disables a plugin; disabling stops its instance. Bundled
    /// and installed plugins keep the choice in `plugins.toml`; enabling an
    /// installed plugin approves its current `plugin.wasm`.
    pub fn set_enabled(&mut self, plugin: &PluginId, enabled: bool) {
        self.stop(plugin);
        let Some(registered) = self.registry.get(plugin) else {
            return;
        };
        match registered.source {
            PluginSource::Dev => {
                if enabled {
                    self.disabled.remove(plugin);
                } else {
                    self.disabled.insert(plugin.clone());
                }
            }
            PluginSource::Bundled => self.states.set(
                plugin,
                PluginState {
                    enabled,
                    sha256: None,
                },
            ),
            PluginSource::Installed => {
                let sha256 = if enabled {
                    wasm_sha256(&registered.entry_path()).ok()
                } else {
                    self.states.get(plugin).and_then(|s| s.sha256.clone())
                };
                let enabled = enabled && sha256.is_some();
                self.states.set(plugin, PluginState { enabled, sha256 });
            }
        }
    }

    /// Whether `plugin` may run (see the module docs for the defaults).
    pub fn is_enabled(&self, plugin: &PluginId) -> bool {
        let state = self.states.get(plugin);
        match self.registry.get(plugin).map(|p| p.source) {
            Some(PluginSource::Bundled) => state.is_none_or(|s| s.enabled),
            Some(PluginSource::Installed) => state.is_some_and(|s| s.enabled && s.sha256.is_some()),
            Some(PluginSource::Dev) | None => !self.disabled.contains(plugin),
        }
    }

    /// The hash activation must find for `plugin` (installed plugins only).
    pub(super) fn approved_sha256(&self, plugin: &RegisteredPlugin) -> Option<String> {
        if plugin.source != PluginSource::Installed {
            return None;
        }
        self.states.get(plugin.id()).and_then(|s| s.sha256.clone())
    }

    /// Records a finished [`install_from_folder`] (run it, and
    /// [`precompile`], off the UI thread first): re-reads the manifests and
    /// sets the plugin's state. A new plugin starts disabled.
    pub fn finish_install(&mut self, report: &InstallReport) {
        self.stop(&report.id);
        let keep = report.replaced.as_ref() == Some(&report.capabilities)
            && self.states.get(&report.id).is_some_and(|s| s.enabled);
        self.states.set(
            &report.id,
            PluginState {
                enabled: keep,
                sha256: keep.then(|| report.sha256.clone()),
            },
        );
        self.rediscover();
    }

    /// Removes an installed plugin ("Uninstall"): stops it, deletes its
    /// folder (only a folder directly in the installed root, never a
    /// bundled or dev one), forgets its `plugins.toml` entry and its grants
    /// (global and every project's, so a later plugin with the same id is
    /// asked again), and re-reads the manifests. Its kv data is kept, as
    /// for a disabled plugin.
    pub fn uninstall(&mut self, plugin: &PluginId) -> Result<(), InstallError> {
        let registered = self
            .registry
            .get(plugin)
            .ok_or_else(|| InstallError::Invalid(format!("no plugin `{plugin}`")))?;
        let root = &self.config.paths.installed;
        if registered.source != PluginSource::Installed
            || registered.dir.parent() != Some(root.as_path())
        {
            return Err(InstallError::Invalid(format!(
                "`{plugin}` is not an installed plugin"
            )));
        }
        let dir = registered.dir.clone();
        self.stop(plugin);
        // `remove_dir_all` removes a symlink itself, never what it points to.
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(InstallError::Io(e.to_string())),
        }
        self.states.remove(plugin);
        if self.config.persist {
            forget_everywhere(&self.config.paths, plugin);
        }
        // This session's answers (and an in-memory store's).
        for (capability, _, _) in self.grants(plugin) {
            self.revoke(plugin, &capability);
        }
        self.rediscover();
        Ok(())
    }

    /// Catches up with what another window or process saved: plugins
    /// installed, uninstalled, enabled or disabled there (`plugins.toml`
    /// and the plugin folders), and grants given or revoked there. Stops
    /// the instances of plugins that are gone, disabled or replaced, and
    /// recycles those whose `/project` grant changed (the next call asks
    /// again if it must). Returns the plugins that were enabled and are
    /// not any more.
    pub fn sync_from_disk(&mut self) -> Vec<PluginId> {
        let before: Vec<_> = self
            .registry
            .plugins()
            .map(|p| (p.clone(), self.is_enabled(p.id()), self.approved_sha256(p)))
            .collect();
        self.states.reload();
        self.rediscover();
        let recycle = {
            let mut store = lock(&self.permissions.store);
            store.refresh();
            store.take_static_changes()
        };
        let mut gone = Vec::new();
        for (old, was_enabled, old_sha) in before {
            let id = old.id();
            let now = self.registry.get(id);
            let replaced =
                now != Some(&old) || now.and_then(|p| self.approved_sha256(p)) != old_sha;
            let runs = now.is_some() && self.is_enabled(id);
            if was_enabled && !runs {
                gone.push(id.clone());
            }
            if replaced || !runs {
                self.stop(id);
            } else if recycle.contains(id)
                && let Some(h) = lock(&self.instances).get(id)
            {
                h.recycle();
            }
        }
        gone
    }

    fn rediscover(&mut self) {
        let roots = self
            .config
            .paths
            .discovery_roots(self.config.dev_folders.clone());
        self.registry = Registry::discover(&roots, &self.config.user_limits);
    }

    /// Blocking convenience for headless use and tests: copy, precompile
    /// and [`PluginManager::finish_install`].
    pub fn install_from_folder(&mut self, src: &Path) -> Result<PluginId, InstallError> {
        let report = install_from_folder(src, &self.config.paths.installed)?;
        precompile(&self.runtime, &report)?;
        self.finish_install(&report);
        Ok(report.id)
    }

    /// Stops `plugin`'s instance, if any.
    pub(super) fn stop(&self, plugin: &PluginId) {
        if let Some(h) = lock(&self.instances).remove(plugin) {
            h.shutdown();
        }
    }
}
