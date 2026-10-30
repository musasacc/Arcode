//! One plugin's instance thread, mailbox and restart logic (§5).
//!
//! An [`InstanceHandle`] costs nothing until its first call: dispatching
//! spawns `plugin-<id>` (2 MiB stack), which activates the plugin and then
//! blocks on its mailbox. After `idle_timeout` without calls and with no
//! open panel it calls `deactivate`, drops the store and exits; the next
//! call starts a new thread. A trap drops the store; the next call
//! re-instantiates with `activate(restore)` and replays `open-view` for
//! every panel still open, subject to the crash policy.

mod serve;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::bindings::PanelId;
use crate::cancel::{CancelToken, lock};
use crate::dispatch::{Call, CallError, CallResult, Dispatch, Mailbox, Pushed};
use crate::engine::Runtime;
use crate::host_impl::kv::KvFiles;
use crate::limits::CrashTracker;
use crate::net::NetworkLog;
use crate::plugin_log::PluginLog;
use crate::registry::RegisteredPlugin;
use crate::state::{InstanceStats, ProjectContext, Services, Shared};
use crate::ui_tree::{PanelSnapshot, PanelTree};

/// Everything needed to create instances of one plugin.
#[derive(Debug, Clone)]
pub struct InstanceSpec {
    /// The plugin (manifest, folder, clamped limits).
    pub plugin: RegisteredPlugin,
    /// The open project, if any.
    pub project: Option<ProjectContext>,
    /// The user's values for the plugin's settings.
    pub user_settings: serde_json::Map<String, serde_json::Value>,
    /// Where the plugin's kv scopes are stored ([`KvFiles::none`]: memory).
    pub kv: KvFiles,
    /// App services.
    pub services: Services,
    /// sha256 of the `plugin.wasm` the user approved (`plugins.toml`);
    /// activation refuses any other file. `None`: not checked (bundled and
    /// dev plugins).
    pub approved_sha256: Option<String>,
}

/// A call in flight: its id, its cancel token and its result.
#[derive(Debug)]
pub struct Pending {
    /// Process-unique call id.
    pub id: u64,
    /// Cancels the call (cooperatively, then hard after the grace period).
    pub cancel: CancelToken,
    /// Resolves once with the result; a GPUI task awaits it.
    pub result: async_channel::Receiver<CallResult>,
}

impl Pending {
    /// Blocks until the result arrives (tests, headless use).
    pub fn wait(&self) -> CallResult {
        self.result
            .recv_blocking()
            .unwrap_or(Err(CallError::Stopped))
    }
}

/// Shared between the handle and its instance thread.
pub(super) struct Inner {
    pub(super) runtime: Arc<Runtime>,
    pub(super) spec: InstanceSpec,
    pub(super) mailbox: Mailbox,
    pub(super) shared: Arc<Shared>,
    pub(super) crash: Mutex<CrashTracker>,
    pub(super) lens_strikes: AtomicU32,
    pub(super) threads: AtomicU32,
    /// Set by [`InstanceHandle::recycle`]: drop the store before the next
    /// call so static grants are evaluated again.
    pub(super) recycle: AtomicBool,
}

/// `host.settings()` text: manifest defaults merged with the user's values
/// for declared keys only.
fn merged_settings(
    spec: &InstanceSpec,
    user: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut settings = spec.plugin.manifest.default_settings();
    for (k, v) in user {
        if settings.contains_key(k) {
            settings.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(settings).to_string()
}

/// The app's handle to one plugin's instance.
#[derive(Clone)]
pub struct InstanceHandle {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for InstanceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceHandle")
            .field("plugin", self.inner.spec.plugin.id())
            .finish_non_exhaustive()
    }
}

impl InstanceHandle {
    /// A handle; no thread, engine or store is created until a call.
    pub fn new(runtime: Arc<Runtime>, spec: InstanceSpec) -> Self {
        let settings = merged_settings(&spec, &spec.user_settings);
        let shared = Shared::new(spec.kv.clone(), settings);
        let mailbox = Mailbox::new(runtime.config().mailbox);
        Self {
            inner: Arc::new(Inner {
                runtime,
                spec,
                mailbox,
                shared: Arc::new(shared),
                crash: Mutex::new(CrashTracker::default()),
                lens_strikes: AtomicU32::new(0),
                threads: AtomicU32::new(0),
                recycle: AtomicBool::new(false),
            }),
        }
    }

    /// The plugin this handle runs.
    pub fn plugin(&self) -> &crate::registry::RegisteredPlugin {
        &self.inner.spec.plugin
    }

    /// Queues `call`, starting the instance thread if needed.
    pub fn dispatch(&self, call: Call, gesture: bool) -> Pending {
        let (d, result) = Dispatch::new(call, gesture);
        self.dispatch_stamped(d, result)
    }

    /// Like [`InstanceHandle::dispatch`] for a command about `doc`
    /// (lens clicks), which `editor = "context"` may then read.
    pub fn dispatch_about(&self, call: Call, gesture: bool, doc: u64) -> Pending {
        let (mut d, result) = Dispatch::new(call, gesture);
        d.origin_doc = Some(doc);
        self.dispatch_stamped(d, result)
    }

    fn dispatch_stamped(
        &self,
        d: Dispatch,
        result: async_channel::Receiver<CallResult>,
    ) -> Pending {
        let pending = Pending {
            id: d.id,
            cancel: d.cancel.clone(),
            result,
        };
        if self.inner.mailbox.push(d) == Pushed::NeedsThread {
            self.spawn();
        }
        pending
    }

    /// Shows a contributed panel or opener: allocates a panel id, records
    /// it as open and dispatches `open-view`.
    pub fn open_view(&self, contribution: &str, args: &str) -> (PanelId, Pending) {
        let panel = self.inner.shared.allocate_panel();
        self.inner.shared.panel_opened(panel, contribution, args);
        let call = Call::OpenView {
            panel,
            contribution: contribution.to_owned(),
            args: args.to_owned(),
        };
        (panel, self.dispatch(call, false))
    }

    /// The user closed a panel: forget it and tell the plugin.
    pub fn close_view(&self, panel: PanelId) -> Pending {
        self.inner.shared.panel_closed(panel);
        self.dispatch(Call::ViewClosed(panel), false)
    }

    /// The current view of `panel` (empty until the plugin renders).
    pub fn panel_view(&self, panel: PanelId) -> Option<Arc<PanelSnapshot>> {
        lock(&self.inner.shared.trees)
            .get(&panel)
            .map(PanelTree::snapshot)
    }

    /// Open panels and their manifest-local contribution ids.
    pub fn open_panels(&self) -> Vec<(PanelId, String)> {
        lock(&self.inner.shared.panels)
            .iter()
            .map(|(id, p)| (*id, p.contribution.clone()))
            .collect()
    }

    /// The plugin's log.
    pub fn log(&self) -> &PluginLog {
        &self.inner.shared.log
    }

    /// Requests the plugin made (method, host, status; no bodies).
    pub fn network_log(&self) -> &NetworkLog {
        &self.inner.shared.network
    }

    /// Replaces the user's values for the plugin's settings; the next
    /// `host.settings()` call sees them.
    pub fn set_user_settings(&self, user: &serde_json::Map<String, serde_json::Value>) {
        *lock(&self.inner.shared.settings) = merged_settings(&self.inner.spec, user);
    }

    /// Drops the current store (after `deactivate`) before the next call,
    /// which re-creates it with `activate(restore)`, asks for the file grant
    /// again and replays open views. Used when a static grant is revoked.
    pub fn recycle(&self) {
        self.inner.recycle.store(true, Ordering::SeqCst);
    }

    /// Forgets recent traps and lens strikes ("Restart" after the plugin
    /// was disabled for the session).
    pub fn reset_crashes(&self) {
        *lock(&self.inner.crash) = CrashTracker::default();
        self.inner.lens_strikes.store(0, Ordering::SeqCst);
    }

    /// Counters (idle-cost assertions, Plugins view).
    pub fn stats(&self) -> &InstanceStats {
        &self.inner.shared.stats
    }

    /// True while an instance thread exists.
    pub fn thread_running(&self) -> bool {
        self.inner.threads.load(Ordering::SeqCst) > 0
    }

    /// True once disabled for the session after repeated traps.
    pub fn is_disabled(&self) -> bool {
        lock(&self.inner.crash).is_disabled()
    }

    /// Stops accepting calls; the thread deactivates and exits.
    pub fn shutdown(&self) {
        self.inner.mailbox.close();
    }

    /// Waits up to `timeout` for the instance thread to exit.
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.thread_running() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    fn spawn(&self) {
        let inner = self.inner.clone();
        inner.threads.fetch_add(1, Ordering::SeqCst);
        InstanceStats::bump(&inner.shared.stats.threads);
        let name = format!("plugin-{}", inner.spec.plugin.id());
        let stack = inner.runtime.config().thread_stack;
        let spawned = std::thread::Builder::new()
            .name(name)
            .stack_size(stack)
            .spawn({
                let inner = inner.clone();
                move || {
                    serve::serve(&inner);
                    inner.threads.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if let Err(e) = spawned {
            log::error!("cannot start plugin thread: {e}");
            inner.threads.fetch_sub(1, Ordering::SeqCst);
            inner.mailbox.close();
        }
    }
}
