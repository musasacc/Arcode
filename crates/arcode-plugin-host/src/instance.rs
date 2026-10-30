//! One plugin's instance thread, mailbox and restart logic (§5).
//!
//! An [`InstanceHandle`] costs nothing until its first call: dispatching
//! spawns `plugin-<id>` (2 MiB stack), which activates the plugin and then
//! blocks on its mailbox. After `idle_timeout` without calls and with no
//! open panel it calls `deactivate`, drops the store and exits; the next
//! call starts a new thread. A trap drops the store; the next call
//! re-instantiates with `activate(restore)` and replays `open-view` for
//! every panel still open, subject to the crash policy.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::bindings::PanelId;
use crate::bindings::guest::Activation;
use crate::cancel::{CancelToken, lock};
use crate::consent::FileGrant;
use crate::dispatch::{Call, CallError, CallResult, Dispatch, Mailbox, Pushed, Recv};
use crate::engine::Runtime;
use crate::limits::{CrashTracker, CrashVerdict};
use crate::live::{Failure, Live, to_result};
use crate::output::PluginOutput;
use crate::plugin_log::PluginLog;
use crate::registry::RegisteredPlugin;
use crate::state::{InstanceCtx, InstanceStats, ProjectContext, Services, Shared};

/// Lens budget overruns after which lenses are off for the session.
const LENS_STRIKES: u32 = 3;

/// Everything needed to create instances of one plugin.
#[derive(Debug, Clone)]
pub struct InstanceSpec {
    /// The plugin (manifest, folder, clamped limits).
    pub plugin: RegisteredPlugin,
    /// The open project, if any.
    pub project: Option<ProjectContext>,
    /// The user's values for the plugin's settings.
    pub user_settings: serde_json::Map<String, serde_json::Value>,
    /// App services.
    pub services: Services,
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
        self.result.recv_blocking().unwrap_or(Err(CallError::Stopped))
    }
}

struct Inner {
    runtime: Arc<Runtime>,
    spec: InstanceSpec,
    settings: String,
    mailbox: Mailbox,
    shared: Arc<Shared>,
    crash: Mutex<CrashTracker>,
    lens_strikes: AtomicU32,
    threads: AtomicU32,
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
        let mut settings = spec.plugin.manifest.default_settings();
        for (k, v) in &spec.user_settings {
            if settings.contains_key(k) {
                settings.insert(k.clone(), v.clone());
            }
        }
        let settings = serde_json::Value::Object(settings).to_string();
        let mailbox = Mailbox::new(runtime.config().mailbox);
        Self {
            inner: Arc::new(Inner {
                runtime,
                spec,
                settings,
                mailbox,
                shared: Arc::new(Shared::default()),
                crash: Mutex::new(CrashTracker::default()),
                lens_strikes: AtomicU32::new(0),
                threads: AtomicU32::new(0),
            }),
        }
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

    fn dispatch_stamped(&self, d: Dispatch, result: async_channel::Receiver<CallResult>) -> Pending {
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
        lock(&self.inner.shared.panels).remove(&panel);
        self.dispatch(Call::ViewClosed(panel), false)
    }

    /// The plugin's log.
    pub fn log(&self) -> &PluginLog {
        &self.inner.shared.log
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
                    serve(&inner);
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

/// The instance thread.
fn serve(inner: &Inner) {
    let stats = &inner.shared.stats;
    let mut live: Option<Live> = None;
    let mut restore = stats.instantiations.load(Ordering::SeqCst) > 0;
    loop {
        let received = inner.mailbox.recv_timeout(inner.runtime.config().idle_timeout);
        InstanceStats::bump(&stats.wakeups);
        match received {
            Recv::Call(d) => handle(inner, &mut live, &mut restore, &d),
            Recv::Timeout if inner.shared.has_open_panels() => {}
            Recv::Timeout => {
                unload(inner, &mut live);
                restore = true;
                if inner.mailbox.retire() {
                    inner.spec.services.output.send(PluginOutput::Unloaded {
                        plugin: inner.spec.plugin.id().clone(),
                    });
                    return;
                }
            }
            Recv::Closed => {
                unload(inner, &mut live);
                inner.mailbox.retire();
                return;
            }
        }
    }
}

fn unload(inner: &Inner, live: &mut Option<Live>) {
    if let Some(mut l) = live.take() {
        l.deactivate(inner.runtime.config().budgets.deactivate);
        inner.shared.stats.loaded.store(false, Ordering::SeqCst);
    }
}

fn handle(inner: &Inner, live: &mut Option<Live>, restore: &mut bool, d: &Dispatch) {
    if let Some(refusal) = refusal(inner, &d.call) {
        d.answer(Err(refusal));
        return;
    }
    if live.is_none() {
        let reason = if *restore {
            Activation::Restore
        } else {
            activation_reason(inner, &d.call)
        };
        match activate(inner, &reason, &d.cancel) {
            Ok(l) => {
                *live = Some(l);
                if *restore {
                    replay_views(inner, live, d);
                }
            }
            Err(error) => {
                d.answer(Err(error));
                return;
            }
        }
    }
    let Some(l) = live.as_mut() else {
        d.answer(Err(CallError::Stopped));
        return;
    };
    let budget = budget_for(inner, &d.call);
    let result = l.call(d, budget);
    if let Err(Failure::Trap { reason, budget }) = &result {
        *live = None;
        *restore = true;
        inner.shared.stats.loaded.store(false, Ordering::SeqCst);
        if *budget && matches!(d.call, Call::CodeLenses(_)) {
            inner.lens_strikes.fetch_add(1, Ordering::SeqCst);
        } else {
            crashed(inner, reason);
        }
    }
    d.answer(to_result(result));
}

/// Calls refused before touching the instance.
fn refusal(inner: &Inner, call: &Call) -> Option<CallError> {
    let crash = lock(&inner.crash);
    if crash.is_disabled() {
        return Some(CallError::Disabled);
    }
    if let Some(left) = crash.backoff_left(Instant::now()) {
        return Some(CallError::Backoff(left));
    }
    let lenses_off = inner.lens_strikes.load(Ordering::SeqCst) >= LENS_STRIKES;
    (lenses_off && matches!(call, Call::CodeLenses(_))).then_some(CallError::LensesDisabled)
}

fn crashed(inner: &Inner, reason: &str) {
    let verdict = lock(&inner.crash).record(Instant::now(), &inner.runtime.config().crash);
    let disabled = verdict == CrashVerdict::Disable;
    let name = &inner.spec.plugin.manifest.plugin.name;
    inner.shared.log.push(
        crate::bindings::Level::Error,
        &format!("{name} stopped: {reason}"),
    );
    inner.spec.services.output.send(PluginOutput::Crashed {
        plugin: inner.spec.plugin.id().clone(),
        reason: reason.to_owned(),
        disabled,
    });
}

fn activation_reason(inner: &Inner, call: &Call) -> Activation {
    let m = &inner.spec.plugin.manifest;
    match call {
        Call::Command { command, .. } => Activation::Command(command.clone()),
        Call::CodeLenses(doc) => Activation::Language(doc.language.clone()),
        Call::OpenView { contribution, .. }
            if m.contributes.openers.iter().any(|o| &o.id == contribution) =>
        {
            Activation::Opener(contribution.clone())
        }
        Call::OpenView { contribution, .. } => Activation::Panel(contribution.clone()),
        _ => Activation::Restore,
    }
}

fn budget_for(inner: &Inner, call: &Call) -> Duration {
    let b = &inner.runtime.config().budgets;
    match call {
        Call::Command { .. } => b.command,
        Call::CodeLenses(_) => b.code_lenses,
        _ => b.view,
    }
}

/// Creates the store: consent for the file grant, component (cache),
/// import check, instantiate, `activate(reason)`.
fn activate(inner: &Inner, reason: &Activation, cancel: &CancelToken) -> Result<Live, CallError> {
    let spec = &inner.spec;
    let runtime = &inner.runtime;
    let manifest = &spec.plugin.manifest;
    let engine = runtime.engine().map_err(CallError::Activation)?;
    let component = runtime
        .cache()
        .load(engine, &spec.plugin.entry_path())
        .map_err(|e| CallError::Activation(e.to_string()))?;
    let imports = crate::imports_check::component_imports(engine, &component);
    crate::imports_check::check_imports(imports.iter().map(String::as_str), &manifest.capabilities)
        .map_err(|e| CallError::Activation(e.to_string()))?;
    let files = match &spec.project {
        Some(p) => spec.services.permissions.file_grant(
            manifest,
            &p.name,
            runtime.network_enabled(),
            cancel,
        ),
        None => FileGrant::None,
    };
    let ctx = Arc::new(InstanceCtx {
        manifest: manifest.clone(),
        limits: spec.plugin.limits,
        project: spec.project.clone(),
        files,
        settings: inner.settings.clone(),
        services: spec.services.clone(),
        shared: inner.shared.clone(),
        runtime: runtime.clone(),
    });
    let mut live = Live::instantiate(ctx, &component).map_err(CallError::Activation)?;
    InstanceStats::bump(&inner.shared.stats.instantiations);
    match live.activate(reason, runtime.config().budgets.activate, cancel) {
        Ok(()) => {
            inner.shared.stats.loaded.store(true, Ordering::SeqCst);
            Ok(live)
        }
        Err(Failure::Guest(m)) => Err(CallError::Activation(m)),
        Err(Failure::Trap { reason, .. }) => {
            crashed(inner, &reason);
            Err(CallError::Activation(reason))
        }
    }
}

/// After a restart, re-sends `open-view` for every panel still open (except
/// the one `current` is about to open).
fn replay_views(inner: &Inner, live: &mut Option<Live>, current: &Dispatch) {
    let skip = match &current.call {
        Call::OpenView { panel, .. } => Some(*panel),
        _ => None,
    };
    let panels: Vec<_> = lock(&inner.shared.panels)
        .iter()
        .filter(|(id, _)| Some(**id) != skip)
        .map(|(id, p)| (*id, p.clone()))
        .collect();
    for (panel, open) in panels {
        let Some(l) = live.as_mut() else { return };
        let call = Call::OpenView {
            panel,
            contribution: open.contribution,
            args: open.args,
        };
        let (d, _rx) = Dispatch::new(call, false);
        if let Err(Failure::Trap { reason, .. }) = l.call(&d, inner.runtime.config().budgets.view) {
            *live = None;
            crashed(inner, &reason);
            return;
        }
    }
}
