//! Data owned by an instance: the wasmtime store data ([`HostState`]), the
//! per-plugin state that survives instance recycling ([`Shared`]) and the
//! services the app provides ([`Services`]).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::bindings::{DocId, PanelId};
use crate::bridge::{EditorBridge, HttpTransport, NoEditor, NoHttp, NoIndex, ProjectIndex};
use crate::budget::SliceBudget;
use crate::cancel::{CancelToken, lock};
use crate::consent::{DenyAll, FileGrant, Permissions};
use crate::engine::Runtime;
use crate::grant_store::GrantStore;
use crate::host_impl::kv::KvStore;
use crate::limits::Limiter;
use crate::manifest::{EffectiveLimits, Manifest};
use crate::output::{OutputSender, PluginOutput};
use crate::plugin_log::PluginLog;

/// The open project, as plugins may see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// Absolute root; preopened as `/project` when granted. Never shown to
    /// the plugin.
    pub root: PathBuf,
    /// Display name (`workspace.root-name`).
    pub name: String,
}

/// What the app provides to every instance.
#[derive(Clone)]
pub struct Services {
    /// Documents.
    pub editor: Arc<dyn EditorBridge>,
    /// The project's file index.
    pub index: Arc<dyn ProjectIndex>,
    /// The network stack used by `http.send`.
    pub http: Arc<dyn HttpTransport>,
    /// Grants and prompts.
    pub permissions: Permissions,
    /// Messages to the app.
    pub output: OutputSender,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

impl Services {
    /// No editor, no index, no network, every prompt dismissed; returns
    /// the output receiver too.
    pub fn headless() -> (Self, async_channel::Receiver<PluginOutput>) {
        let (output, rx) = OutputSender::channel();
        let services = Self {
            editor: Arc::new(NoEditor),
            index: Arc::new(NoIndex),
            http: Arc::new(NoHttp),
            permissions: Permissions::new(GrantStore::in_memory(true), Arc::new(DenyAll)),
            output,
        };
        (services, rx)
    }
}

/// A panel the host considers open, with what is needed to restore it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPanel {
    /// Manifest-local panel or opener id.
    pub contribution: String,
    /// `open-view` args to replay on restore.
    pub args: String,
}

/// Counters for tests and the Plugins view.
#[derive(Debug, Default)]
pub struct InstanceStats {
    /// Times the instance thread returned from waiting on its mailbox.
    pub wakeups: AtomicU64,
    /// Instances created (activations, including restarts).
    pub instantiations: AtomicU64,
    /// Instance threads started.
    pub threads: AtomicU64,
    /// True while an instance (store) exists.
    pub loaded: AtomicBool,
    /// True while an instance thread exists.
    pub running: AtomicBool,
    /// Peak linear memory of the last instance, in bytes.
    pub peak_memory: AtomicU64,
}

impl InstanceStats {
    pub(crate) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

/// Per-plugin state that outlives instance recycling.
#[derive(Debug)]
pub struct Shared {
    /// The plugin log (stdout, stderr, `host.log`).
    pub log: PluginLog,
    /// kv storage.
    pub kv: Mutex<KvStore>,
    /// Open panels by id.
    pub panels: Mutex<BTreeMap<PanelId, OpenPanel>>,
    /// Next panel id.
    pub next_panel: AtomicU32,
    /// Times of recent `host.notify` calls (5 per minute).
    pub notify_times: Mutex<VecDeque<Instant>>,
    /// Counters.
    pub stats: InstanceStats,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            log: PluginLog::new(),
            kv: Mutex::new(KvStore::default()),
            panels: Mutex::new(BTreeMap::new()),
            next_panel: AtomicU32::new(1),
            notify_times: Mutex::new(VecDeque::new()),
            stats: InstanceStats::default(),
        }
    }
}

impl Shared {
    /// A fresh panel id, unique for this plugin.
    pub fn allocate_panel(&self) -> PanelId {
        self.next_panel.fetch_add(1, Ordering::SeqCst)
    }

    /// Records an open panel.
    pub fn panel_opened(&self, panel: PanelId, contribution: &str, args: &str) {
        lock(&self.panels).insert(
            panel,
            OpenPanel {
                contribution: contribution.to_owned(),
                args: args.to_owned(),
            },
        );
    }

    /// True if any panel is open (blocks idle unload).
    pub fn has_open_panels(&self) -> bool {
        !lock(&self.panels).is_empty()
    }
}

/// Immutable context of one instance.
pub(crate) struct InstanceCtx {
    pub manifest: Manifest,
    pub limits: EffectiveLimits,
    pub project: Option<ProjectContext>,
    pub files: FileGrant,
    /// `host.settings()`: manifest defaults merged with user values.
    pub settings: String,
    pub services: Services,
    pub shared: Arc<Shared>,
    pub runtime: Arc<Runtime>,
}

impl InstanceCtx {
    pub fn output(&self, output: PluginOutput) {
        self.services.output.send(output);
    }
}

/// The call being executed.
#[derive(Debug, Clone)]
pub(crate) struct CallState {
    pub id: u64,
    pub gesture: bool,
    pub cancel: CancelToken,
    pub budget: SliceBudget,
    pub origin_doc: Option<DocId>,
    pub task_started: bool,
    pub network_requests: u32,
}

impl CallState {
    pub fn new(
        id: u64,
        gesture: bool,
        budget: Duration,
        origin_doc: Option<DocId>,
        cancel: CancelToken,
    ) -> Self {
        Self {
            id,
            gesture,
            cancel,
            budget: SliceBudget::new(budget),
            origin_doc,
            task_started: false,
            network_requests: 0,
        }
    }
}

/// The data of a plugin's `Store`.
pub(crate) struct HostState {
    pub wasi: WasiCtx,
    pub table: ResourceTable,
    pub limiter: Limiter,
    pub ctx: Arc<InstanceCtx>,
    pub call: CallState,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
