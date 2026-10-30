//! The instance thread: mailbox loop, activation, calls, traps and
//! restore (§5 "Instance thread", "Crashes").

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::bindings::guest::Activation;
use crate::cancel::{CancelToken, lock};
use crate::consent::FileGrant;
use crate::dispatch::{Call, CallError, Dispatch, Recv};
use crate::limits::CrashVerdict;
use crate::live::{Failure, Live, to_result};
use crate::output::PluginOutput;
use crate::state::{InstanceCtx, InstanceStats};

use super::Inner;

/// Lens budget overruns after which lenses are off for the session.
const LENS_STRIKES: u32 = 3;

/// The instance thread.
pub(super) fn serve(inner: &Inner) {
    let stats = &inner.shared.stats;
    let mut live: Option<Live> = None;
    let mut restore = stats.instantiations.load(Ordering::SeqCst) > 0;
    loop {
        let received = inner
            .mailbox
            .recv_timeout(inner.runtime.config().idle_timeout);
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
    if inner.recycle.swap(false, Ordering::SeqCst) && live.is_some() {
        unload(inner, live);
        *restore = true;
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
    let trap = match &result {
        Err(Failure::Trap { reason, budget }) => Some((reason.clone(), *budget)),
        _ => None,
    };
    let Some((reason, over_budget)) = trap else {
        d.answer(to_result(result));
        return;
    };
    // A trap poisons the store. Bookkeeping first (so a racing call sees
    // the backoff), then answer, then drop the store (unmapping memory
    // can take a moment).
    let dead = live.take();
    *restore = true;
    inner.shared.stats.loaded.store(false, Ordering::SeqCst);
    if over_budget && matches!(d.call, Call::CodeLenses(_)) {
        inner.lens_strikes.fetch_add(1, Ordering::SeqCst);
    } else {
        crashed(inner, &reason);
    }
    d.answer(to_result(result));
    drop(dead);
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
        .load_approved(
            engine,
            &spec.plugin.entry_path(),
            spec.approved_sha256.as_deref(),
        )
        .map_err(|e| {
            if e == crate::cache::LoadError::Changed {
                spec.services.output.send(PluginOutput::NeedsApproval {
                    plugin: manifest.id().clone(),
                });
            }
            CallError::Activation(e.to_string())
        })?;
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
