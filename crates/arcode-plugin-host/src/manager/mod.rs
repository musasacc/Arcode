//! [`PluginManager`]: the one type the app talks to (§12 `lib.rs` facade).
//!
//! Channels and traits only, no GPUI. The app:
//! - implements [`EditorBridge`], [`ConsentProvider`] and optionally
//!   [`ProjectIndex`] / [`HttpTransport`] and passes them as [`AppServices`];
//! - drains the [`PluginOutput`] receiver returned by [`PluginManager::new`]
//!   on a GPUI task;
//! - calls the methods in `calls.rs` (run a command, show a panel, forward
//!   UI events, fetch rows, ask for code lenses), each of which returns
//!   immediately with a [`Pending`] whose `result` channel a GPUI task
//!   awaits. Nothing here blocks on a plugin.
//!
//! Creating the manager reads `plugin.toml` files only: no engine, no
//! thread, no wasm. A plugin's instance starts on its first call.
//!
//! See `crates/arcode-plugin-host/README.md` for the integration guide.
//!
//! [`EditorBridge`]: crate::bridge::EditorBridge
//! [`ProjectIndex`]: crate::bridge::ProjectIndex
//! [`HttpTransport`]: crate::bridge::HttpTransport
//! [`ConsentProvider`]: crate::consent::ConsentProvider
//! [`PluginOutput`]: crate::output::PluginOutput
//! [`Pending`]: crate::instance::Pending

mod calls;
mod enable;

pub use calls::{LensCall, PanelCall, event_is_gesture};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::bridge::{EditorBridge, HttpTransport, NoIndex, ProjectIndex};
use crate::cancel::lock;
use crate::consent::{ChannelConsent, ConsentProvider, Permissions};
use crate::engine::{Runtime, RuntimeConfig};
use crate::grant_store::GrantStore;
use crate::grants::{Capability, GrantScope, GrantState};
use crate::host_impl::kv::KvFiles;
use crate::index::WalkIndex;
use crate::instance::{InstanceHandle, InstanceSpec};
use crate::manifest::{PluginId, QualifiedId, UserLimits};
use crate::output::{OutputSender, PluginOutput};
use crate::paths::PluginPaths;
use crate::plugin_state::PluginStates;
use crate::registry::{RegisteredPlugin, Registry};
use crate::state::{ProjectContext, Services};

/// How the manager is set up.
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// Where plugins, grants, kv data and the `.cwasm` cache live.
    pub paths: PluginPaths,
    /// Engine tunables; `cache_dir` should be `paths.cache`.
    pub runtime: RuntimeConfig,
    /// `settings.plugins.max-memory-mb` and friends.
    pub user_limits: UserLimits,
    /// "Plugins: Load Unpacked…" folders for this session.
    pub dev_folders: Vec<PathBuf>,
    /// Write grants and kv data to `paths` (false keeps them in memory).
    pub persist: bool,
}

impl ManagerConfig {
    /// Defaults for `paths`, persisting to disk.
    pub fn new(paths: PluginPaths) -> Self {
        Self {
            runtime: RuntimeConfig::new(paths.cache.clone()),
            paths,
            user_limits: UserLimits::default(),
            dev_folders: Vec::new(),
            persist: true,
        }
    }
}

/// What the app provides.
#[derive(Clone)]
pub struct AppServices {
    /// Document snapshots and editor writes.
    pub editor: Arc<dyn EditorBridge>,
    /// The project's file index; `None` walks the project tree on demand
    /// ([`WalkIndex`]).
    pub index: Option<Arc<dyn ProjectIndex>>,
    /// The network stack ([`crate::net::UreqTransport`] in the app).
    pub http: Arc<dyn HttpTransport>,
    /// Host-drawn consent prompts; `None` sends them as
    /// [`PluginOutput::Consent`] events ([`ChannelConsent`]).
    pub consent: Option<Arc<dyn ConsentProvider>>,
}

impl std::fmt::Debug for AppServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppServices").finish_non_exhaustive()
    }
}

/// Why the manager refused a call without dispatching it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerError {
    /// No such plugin is registered.
    UnknownPlugin(String),
    /// No such command, panel or opener.
    UnknownContribution(String),
    /// The user disabled the plugin.
    Disabled(PluginId),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPlugin(id) => write!(f, "no plugin `{id}`"),
            Self::UnknownContribution(id) => write!(f, "no contribution `{id}`"),
            Self::Disabled(id) => write!(f, "plugin `{id}` is disabled"),
        }
    }
}

impl std::error::Error for ManagerError {}

/// The plugin system, as seen by the app.
pub struct PluginManager {
    config: ManagerConfig,
    runtime: Arc<Runtime>,
    registry: Registry,
    app: AppServices,
    consent: Arc<dyn ConsentProvider>,
    output: OutputSender,
    project: Option<ProjectContext>,
    permissions: Permissions,
    instances: Mutex<HashMap<PluginId, InstanceHandle>>,
    user_settings: HashMap<PluginId, serde_json::Map<String, serde_json::Value>>,
    /// Dev plugins disabled for this session.
    disabled: HashSet<PluginId>,
    /// `plugins.toml`: enabled state and approved hashes.
    states: PluginStates,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugins", &self.registry.plugins().count())
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

impl PluginManager {
    /// Discovers plugins (manifests only) and returns the manager plus the
    /// receiver of every [`PluginOutput`].
    pub fn new(
        config: ManagerConfig,
        app: AppServices,
    ) -> (Self, async_channel::Receiver<PluginOutput>) {
        let (output, rx) = OutputSender::channel();
        let consent = app
            .consent
            .clone()
            .unwrap_or_else(|| Arc::new(ChannelConsent::new(output.clone())));
        let roots = config.paths.discovery_roots(config.dev_folders.clone());
        let registry = Registry::discover(&roots, &config.user_limits);
        let runtime = Arc::new(Runtime::new(config.runtime.clone()));
        let permissions = Permissions::new(grant_store(&config, None), consent.clone());
        let states = PluginStates::load(config.persist.then(|| config.paths.state_file.clone()));
        let manager = Self {
            config,
            runtime,
            registry,
            app,
            consent,
            output,
            project: None,
            permissions,
            instances: Mutex::new(HashMap::new()),
            user_settings: HashMap::new(),
            disabled: HashSet::new(),
            states,
        };
        (manager, rx)
    }

    /// Every discovered plugin, its contributions and discovery problems.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The runtime (engine counters, cache, master network switch).
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// Re-reads every manifest ("Plugins: Reload"), with `dev_folders` as
    /// the unpacked plugins. Running instances are stopped.
    pub fn reload(&mut self, dev_folders: Vec<PathBuf>) {
        self.stop_all();
        self.config.dev_folders = dev_folders;
        let roots = self
            .config
            .paths
            .discovery_roots(self.config.dev_folders.clone());
        self.registry = Registry::discover(&roots, &self.config.user_limits);
    }

    /// Opens, switches or closes the project. Instances are stopped (their
    /// `/project` preopen, project grants and project kv change).
    pub fn set_project(&mut self, project: Option<ProjectContext>) {
        self.stop_all();
        let root = project.as_ref().map(|p| p.root.as_path());
        self.permissions = Permissions::new(grant_store(&self.config, root), self.consent.clone());
        self.project = project;
    }

    /// The open project.
    pub fn project(&self) -> Option<&ProjectContext> {
        self.project.as_ref()
    }

    /// `settings.plugins.network`: false denies all plugin networking.
    pub fn set_network_enabled(&self, enabled: bool) {
        self.runtime.set_network_enabled(enabled);
    }

    /// The user's values for a plugin's settings (undeclared keys ignored).
    pub fn set_user_settings(
        &mut self,
        plugin: &PluginId,
        values: serde_json::Map<String, serde_json::Value>,
    ) {
        if let Some(h) = lock(&self.instances).get(plugin) {
            h.set_user_settings(&values);
        }
        self.user_settings.insert(plugin.clone(), values);
    }

    /// Every stored grant of `plugin` (Plugins → plugin → Permissions).
    pub fn grants(&self, plugin: &PluginId) -> Vec<(Capability, GrantState, GrantScope)> {
        lock(&self.permissions.store).grants().list(plugin)
    }

    /// Revokes a grant. Revoking a file grant recycles the instance, so
    /// the next call asks again before any guest code runs.
    pub fn revoke(&self, plugin: &PluginId, capability: &Capability) -> bool {
        let removed = lock(&self.permissions.store).revoke(plugin, capability);
        if removed
            && capability.is_static()
            && let Some(h) = lock(&self.instances).get(plugin)
        {
            h.recycle();
        }
        removed
    }

    /// True while any plugin holds a background-network grant (the
    /// status-bar dot).
    pub fn background_network_active(&self) -> bool {
        lock(&self.permissions.store)
            .grants()
            .any_background_network()
    }

    /// The instance handle of `plugin` (log, network log, counters, open
    /// panels). Creating it costs nothing; no thread starts.
    pub fn instance(&self, plugin: &PluginId) -> Result<InstanceHandle, ManagerError> {
        let registered = self
            .registry
            .get(plugin)
            .ok_or_else(|| ManagerError::UnknownPlugin(plugin.to_string()))?;
        if !self.is_enabled(plugin) {
            return Err(ManagerError::Disabled(plugin.clone()));
        }
        let mut instances = lock(&self.instances);
        let handle = instances
            .entry(plugin.clone())
            .or_insert_with(|| InstanceHandle::new(self.runtime.clone(), self.spec(registered)));
        Ok(handle.clone())
    }

    /// Forgets recent crashes so a plugin disabled for the session can run
    /// again ("Restart").
    pub fn restart(&self, plugin: &PluginId) -> Result<(), ManagerError> {
        self.instance(plugin).map(|h| h.reset_crashes())
    }

    /// Stops every instance (app exit, project switch).
    pub fn shutdown(&self) {
        self.stop_all();
    }

    fn stop_all(&self) {
        for (_, h) in lock(&self.instances).drain() {
            h.shutdown();
        }
    }

    fn spec(&self, plugin: &RegisteredPlugin) -> InstanceSpec {
        let root = self.project.as_ref().map(|p| p.root.as_path());
        let index: Arc<dyn ProjectIndex> = match (&self.app.index, root) {
            (Some(index), _) => index.clone(),
            (None, Some(root)) => Arc::new(WalkIndex::new(root)),
            (None, None) => Arc::new(NoIndex),
        };
        let id = plugin.id().to_string();
        InstanceSpec {
            plugin: plugin.clone(),
            project: self.project.clone(),
            user_settings: self
                .user_settings
                .get(plugin.id())
                .cloned()
                .unwrap_or_default(),
            kv: if self.config.persist {
                KvFiles::for_plugin(&self.config.paths, &id, root)
            } else {
                KvFiles::none()
            },
            services: Services {
                editor: self.app.editor.clone(),
                index,
                http: self.app.http.clone(),
                permissions: self.permissions.clone(),
                output: self.output.clone(),
            },
            approved_sha256: self.approved_sha256(plugin),
        }
    }

    /// The plugin behind a namespaced contribution id.
    fn owner(&self, id: &QualifiedId) -> Result<PluginId, ManagerError> {
        PluginId::parse(id.plugin())
            .ok()
            .filter(|p| self.registry.get(p).is_some())
            .ok_or_else(|| ManagerError::UnknownContribution(id.to_string()))
    }
}

fn grant_store(config: &ManagerConfig, root: Option<&std::path::Path>) -> GrantStore {
    if config.persist {
        GrantStore::open(&config.paths, root)
    } else {
        GrantStore::in_memory(root.is_some())
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}
