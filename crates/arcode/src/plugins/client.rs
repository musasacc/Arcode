//! [`PluginClient`]: the workspace's handle to its [`PluginManager`],
//! created lazily.
//!
//! Creating the client costs nothing: no manifest is read. The manager is
//! built on first use ([`PluginClient::ensure`]), which reads the
//! `plugin.toml` files only; the wasmtime engine is created by the host on
//! the first plugin *activation* (a command, a lens language becoming
//! visible, an opener, a shown panel). The workspace calls `ensure` shortly
//! after its first frame so palette commands and sidebar entries exist,
//! and whenever it needs plugins before that.

use std::cell::{Ref, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use arcode_plugin_host::install::{InstallError, InstallReport};
use arcode_plugin_host::{
    AppServices, ManagerConfig, PluginId, PluginManager, PluginOutput, ProjectContext, Runtime,
};

use super::bridge::{AppEditor, AppIndex, EditorRequest};
use super::consent::{AppConsent, ConsentAsk};
pub use super::contributions::{Contributions, PaletteCommand, SidebarPanel};

struct Inner {
    manager: Option<PluginManager>,
    /// The output receiver until the workspace takes it.
    outputs: Option<async_channel::Receiver<PluginOutput>>,
    contributions: Contributions,
    /// Plugins are unavailable (no config or data folder).
    failed: bool,
    /// Consent prompts until the workspace takes the receiver.
    consents: Option<async_channel::Receiver<ConsentAsk>>,
}

/// Cheap to clone; every clone shares one manager. UI thread only.
#[derive(Clone)]
pub struct PluginClient {
    inner: Rc<RefCell<Inner>>,
    editor: Arc<AppEditor>,
    index: Arc<AppIndex>,
    consent: Arc<AppConsent>,
}

impl PluginClient {
    /// A client with no manager yet, plus the receiver of editor writes
    /// plugins ask for.
    pub fn new() -> (Self, async_channel::Receiver<EditorRequest>) {
        let (tx, rx) = async_channel::unbounded();
        let (consent, consents) = AppConsent::new();
        let client = Self {
            inner: Rc::new(RefCell::new(Inner {
                manager: None,
                outputs: None,
                contributions: Contributions::default(),
                failed: false,
                consents: Some(consents),
            })),
            editor: Arc::new(AppEditor::new(tx)),
            index: Arc::new(AppIndex::default()),
            consent: Arc::new(consent),
        };
        (client, rx)
    }

    /// The editor bridge (snapshots, focus).
    pub fn editor(&self) -> &Arc<AppEditor> {
        &self.editor
    }

    /// The file index plugins search.
    pub fn index(&self) -> &Arc<AppIndex> {
        &self.index
    }

    /// Whether the manager exists.
    pub fn is_loaded(&self) -> bool {
        self.inner.borrow().manager.is_some()
    }

    /// Creates the manager (reads manifests; no engine) unless it exists.
    /// Returns true when it was created by this call.
    pub fn ensure(&self) -> bool {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.manager.is_some() || inner.failed {
                return false;
            }
            if super::paths::current().is_none() {
                log::warn!("plugins: no config or data folder; plugins are off");
                inner.failed = true;
                return false;
            }
        }
        let paths = super::paths::current().expect("checked above");
        self.ensure_with(paths, super::paths::dev_folders_from_env(), true)
    }

    /// [`PluginClient::ensure`] with explicit locations (tests); `persist`
    /// false keeps grants, kv and `plugins.toml` in memory.
    pub fn ensure_with(
        &self,
        paths: arcode_plugin_host::paths::PluginPaths,
        dev: Vec<PathBuf>,
        persist: bool,
    ) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.manager.is_some() {
            return false;
        }
        let mut config = ManagerConfig::new(paths);
        config.dev_folders = dev;
        config.persist = persist;
        config.runtime.builtin_languages = arcode_syntax::all_languages()
            .iter()
            .map(|l| l.id.to_owned())
            .collect();
        let services = AppServices {
            editor: self.editor.clone(),
            index: Some(self.index.clone()),
            http: Arc::new(super::bridge::LazyHttp::default()),
            consent: Some(self.consent.clone()),
        };
        let (manager, outputs) = PluginManager::new(config, services);
        for problem in manager.registry().problems() {
            log::warn!(
                "plugin folder {} skipped: {}",
                problem.dir.display(),
                problem.error
            );
        }
        inner.contributions = Contributions::of(&manager);
        inner.manager = Some(manager);
        inner.outputs = Some(outputs);
        true
    }

    /// The consent prompts receiver, once (the workspace shows them).
    pub fn take_consents(&self) -> Option<async_channel::Receiver<ConsentAsk>> {
        self.inner.borrow_mut().consents.take()
    }

    /// The output receiver, once (the workspace drains it).
    pub fn take_outputs(&self) -> Option<async_channel::Receiver<PluginOutput>> {
        self.inner.borrow_mut().outputs.take()
    }

    /// The manager, if loaded. Do not hold the borrow across calls that
    /// may come back here.
    pub fn manager(&self) -> Option<Ref<'_, PluginManager>> {
        Ref::filter_map(self.inner.borrow(), |i| i.manager.as_ref()).ok()
    }

    /// What the registry contributes (empty before loading).
    pub fn contributions(&self) -> Contributions {
        self.inner.borrow().contributions.clone()
    }

    /// Mutable access (project switch, reload).
    fn with_mut<R>(&self, f: impl FnOnce(&mut PluginManager) -> R) -> Option<R> {
        self.inner.borrow_mut().manager.as_mut().map(f)
    }

    /// Tells the manager which project is open (stops instances when it
    /// changes). Does nothing before the manager exists or when the
    /// project is the same. True when it stopped every instance.
    pub fn set_project(&self, root: Option<PathBuf>, name: Option<String>) -> bool {
        self.index.set_root(root.clone());
        let current = self
            .manager()
            .and_then(|m| m.project().map(|p| p.root.clone()));
        if current == root || !self.is_loaded() {
            return false;
        }
        let project = root.map(|root| ProjectContext {
            name: name.unwrap_or_else(|| {
                root.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
            root,
        });
        self.with_mut(|m| m.set_project(project)).is_some()
    }

    /// Re-reads every manifest with `dev` as the unpacked plugins.
    pub fn reload(&self, dev: Vec<PathBuf>) {
        self.with_mut(|m| m.reload(dev));
        let contributions = self.manager().map(|m| Contributions::of(&m));
        if let Some(c) = contributions {
            self.inner.borrow_mut().contributions = c;
        }
    }

    /// Enables or disables `plugin` (see `PluginManager::set_enabled`)
    /// and re-reads what the enabled plugins contribute.
    pub fn set_enabled(&self, plugin: &PluginId, enabled: bool) {
        self.with_mut(|m| m.set_enabled(plugin, enabled));
        self.refresh_contributions();
    }

    /// Removes an installed plugin (folder, state, grants).
    pub fn uninstall(&self, plugin: &PluginId) -> Result<(), InstallError> {
        let result = self
            .with_mut(|m| m.uninstall(plugin))
            .unwrap_or_else(|| Err(InstallError::Invalid("plugins are not loaded".into())));
        self.refresh_contributions();
        result
    }

    /// Records an install that ran in the background (the new plugin is
    /// disabled until enabled).
    pub fn finish_install(&self, report: &InstallReport) {
        self.with_mut(|m| m.finish_install(report));
        self.refresh_contributions();
    }

    /// What a background install needs: the runtime (for `precompile`)
    /// and the installed-plugins folder.
    pub fn install_target(&self) -> Option<(Arc<Runtime>, PathBuf)> {
        let manager = self.manager()?;
        Some((manager.runtime().clone(), manager.paths().installed.clone()))
    }

    /// Catches up with plugin changes another window or process saved
    /// (see `PluginManager::sync_from_disk`). Returns the plugins that were
    /// enabled and are not any more.
    pub fn sync_from_disk(&self) -> Vec<PluginId> {
        let gone = self.with_mut(|m| m.sync_from_disk()).unwrap_or_default();
        self.refresh_contributions();
        gone
    }

    /// `settings.plugins.network`: false refuses every plugin request.
    pub fn set_network_enabled(&self, enabled: bool) {
        if let Some(m) = self.manager() {
            m.set_network_enabled(enabled);
        }
    }

    fn refresh_contributions(&self) {
        let contributions = self.manager().map(|m| Contributions::of(&m));
        if let Some(c) = contributions {
            self.inner.borrow_mut().contributions = c;
        }
    }

    /// The display name of `plugin`, or its id.
    pub fn plugin_name(&self, plugin: &PluginId) -> String {
        self.manager()
            .and_then(|m| {
                m.registry()
                    .get(plugin)
                    .map(|p| p.manifest.plugin.name.clone())
            })
            .unwrap_or_else(|| plugin.to_string())
    }

    /// True once some plugin made the host create the wasmtime engine.
    pub fn engine_created(&self) -> bool {
        self.manager().is_some_and(|m| m.runtime().engine_created())
    }

    /// Stops every instance (window closing).
    pub fn shutdown(&self) {
        if let Some(m) = self.manager() {
            m.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use arcode_plugin_host::paths::PluginPaths;

    use super::*;

    const MANIFEST: &str = r#"
[plugin]
id = "acme.tools"
name = "Acme Tools"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"

[capabilities]
editor = "context"

[[contributes.languages]]
id = "acme"
name = "Acme"
extensions = ["ACME"]

[[contributes.commands]]
id = "run"
title = "Acme: Run"
when = "language == acme"

[[contributes.commands]]
id = "other"
title = "Other"

[[contributes.code-lenses]]
languages = ["acme"]

[[contributes.panels]]
id = "side"
title = "Acme Side"
location = "sidebar"
icon = "database"

[[contributes.panels]]
id = "view"
title = "Acme View"
location = "editor-beside"
"#;

    fn paths(root: &Path) -> PluginPaths {
        let bundled = root.join("bundled");
        let plugin = bundled.join("acme.tools");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("plugin.toml"), MANIFEST).unwrap();
        // Never read: discovery looks at manifests only.
        std::fs::write(plugin.join("plugin.wasm"), b"not wasm").unwrap();
        let mut p = super::super::paths::plugin_paths(
            None,
            &root.join("config"),
            &root.join("data"),
            &root.join("cache"),
            None,
        );
        p.bundled = vec![bundled];
        p
    }

    #[test]
    fn loading_reads_manifests_only() {
        let dir = tempfile::tempdir().unwrap();
        let (client, _requests) = PluginClient::new();
        assert!(!client.is_loaded());
        assert!(client.contributions().commands.is_empty());
        assert!(client.ensure_with(paths(dir.path()), Vec::new(), false));
        assert!(!client.ensure_with(paths(dir.path()), Vec::new(), false));
        assert!(client.is_loaded());
        let c = client.contributions();
        let titles: Vec<&str> = c.commands.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Acme: Run", "Other"]);
        assert!(c.commands[0].when.is_some());
        assert_eq!(c.sidebar.len(), 1);
        assert_eq!(c.sidebar[0].id.as_str(), "acme.tools/side");
        assert_eq!(c.languages[0].extension, "acme");
        // A project switch and the registry queries the app makes create no
        // engine and start no plugin thread.
        client.set_project(Some(dir.path().to_path_buf()), Some("p".into()));
        let manager = client.manager().unwrap();
        assert_eq!(manager.registry().lens_providers("acme").len(), 1);
        assert!(manager.opener_for(Path::new("a.db")).is_none());
        let plugin = PluginId::parse("acme.tools").unwrap();
        assert!(!manager.instance(&plugin).unwrap().thread_running());
        drop(manager);
        assert!(!client.engine_created());
        assert_eq!(client.plugin_name(&plugin), "Acme Tools");
        assert!(client.take_outputs().is_some());
        assert!(client.take_outputs().is_none());
    }
}
