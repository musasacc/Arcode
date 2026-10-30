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
//!
//! [`PluginOutput::NeedsApproval`]: crate::output::PluginOutput::NeedsApproval

use std::path::Path;

use super::PluginManager;
use crate::cache::wasm_sha256;
use crate::cancel::lock;
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
