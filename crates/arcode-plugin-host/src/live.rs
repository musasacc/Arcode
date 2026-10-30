//! A live instance: one wasmtime `Store` plus the instantiated world, and
//! the code that runs one export call under a CPU budget (§5).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use wasmtime::component::Component;
use wasmtime::{Store, UpdateDeadline};

use crate::bindings::PluginWorld;
use crate::bindings::guest::Activation;
use crate::budget::Interrupt;
use crate::cancel::CancelToken;
use crate::dispatch::{Call, CallOutput, CallResult, CallError, Dispatch};
use crate::limits::Limiter;
use crate::output::PluginOutput;
use crate::state::{CallState, HostState, InstanceCtx};

/// How a call into the guest failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The guest trapped; the store is poisoned. `budget` is set when the
    /// CPU budget ran out (used for lens strikes).
    Trap { reason: String, budget: bool },
    /// The guest returned an error string.
    Guest(String),
}

pub(crate) struct Live {
    store: Store<HostState>,
    world: PluginWorld,
}

impl Live {
    /// Creates the store and instantiates `component`.
    pub fn instantiate(ctx: Arc<InstanceCtx>, component: &Component) -> Result<Self, String> {
        let runtime = ctx.runtime.clone();
        let engine = runtime.engine()?;
        let linker = runtime.linker()?;
        let root = ctx.project.as_ref().map(|p| p.root.as_path());
        let wasi = crate::wasi::build(&ctx.shared.log, root, ctx.files)
            .map_err(|e| format!("cannot set up the sandbox: {e}"))?;
        let limiter = Limiter::new(ctx.limits.memory_bytes, &runtime.config().store);
        let budgets = runtime.config().budgets;
        let state = HostState {
            wasi,
            table: wasmtime::component::ResourceTable::new(),
            limiter,
            ctx,
            call: CallState::new(0, false, budgets.activate, None, CancelToken::new()),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limiter);
        let grace = budgets.cancel_grace;
        store.epoch_deadline_callback(move |mut s| {
            let data = s.data_mut();
            let cancel = data.call.cancel.clone();
            data.call
                .budget
                .charge(&cancel, grace)
                .map_err(wasmtime::Error::new)?;
            Ok(UpdateDeadline::Continue(1))
        });
        // Instantiation may run guest code (initializers) under the
        // activation budget.
        store.set_epoch_deadline(1);
        let ticker = runtime.ticker()?;
        let _flight = ticker.enter();
        let world = PluginWorld::instantiate(&mut store, component, linker)
            .map_err(|e| format!("cannot instantiate: {e}"))?;
        Ok(Self { store, world })
    }

    /// Runs `f` as one call with `call` as its context.
    fn run<R>(
        &mut self,
        call: CallState,
        f: impl FnOnce(&PluginWorld, &mut Store<HostState>) -> wasmtime::Result<R>,
    ) -> Result<R, Failure> {
        self.store.data_mut().call = call;
        self.store.set_epoch_deadline(1);
        let runtime = self.store.data().ctx.runtime.clone();
        let result = {
            let _flight = runtime.ticker().ok().map(|t| t.enter());
            f(&self.world, &mut self.store)
        };
        let data = self.store.data();
        let stats = &data.ctx.shared.stats;
        let peak = u64::try_from(data.limiter.peak_memory()).unwrap_or(u64::MAX);
        stats.peak_memory.fetch_max(peak, Ordering::SeqCst);
        if data.call.task_started {
            data.ctx.output(PluginOutput::TaskFinished {
                plugin: data.ctx.manifest.id().clone(),
                call: data.call.id,
            });
        }
        result.map_err(|e| self.classify(&e))
    }

    fn classify(&self, e: &wasmtime::Error) -> Failure {
        if let Some(i) = e.downcast_ref::<Interrupt>() {
            return Failure::Trap {
                reason: i.to_string(),
                budget: matches!(i, Interrupt::Budget(_)),
            };
        }
        if self.store.data().limiter.memory_denied() {
            return Failure::Trap {
                reason: format!(
                    "out of memory (limit {} MiB)",
                    self.store.data().ctx.limits.memory_bytes / (1024 * 1024)
                ),
                budget: false,
            };
        }
        let reason = match e.downcast_ref::<wasmtime::Trap>() {
            Some(trap) => format!("crashed ({trap})"),
            None => format!("crashed ({e})"),
        };
        Failure::Trap {
            reason,
            budget: false,
        }
    }

    /// `activate(reason)`.
    pub fn activate(&mut self, reason: &Activation, budget: Duration, cancel: &CancelToken) -> Result<(), Failure> {
        let call = CallState::new(0, false, budget, None, cancel.clone());
        let r = self.run(call, |w, s| w.arcode_plugin_guest().call_activate(s, reason))?;
        r.map_err(Failure::Guest)
    }

    /// `deactivate()`; failures only get logged.
    pub fn deactivate(&mut self, budget: Duration) {
        let call = CallState::new(0, false, budget, None, CancelToken::new());
        if let Err(e) = self.run(call, |w, s| w.arcode_plugin_guest().call_deactivate(s)) {
            log::debug!("deactivate failed: {e:?}");
        }
    }

    /// Runs one dispatched call.
    pub fn call(&mut self, d: &Dispatch, budget: Duration) -> Result<CallOutput, Failure> {
        let call = CallState::new(d.id, d.gesture, budget, d.origin_doc, d.cancel.clone());
        let done = |r: Result<(), String>| r.map(|()| CallOutput::Done).map_err(Failure::Guest);
        match &d.call {
            Call::Command { command, args } => done(self.run(call, |w, s| {
                w.arcode_plugin_guest().call_handle_command(s, command, args)
            })?),
            Call::CodeLenses(doc) => self
                .run(call, |w, s| w.arcode_plugin_guest().call_provide_code_lenses(s, doc))
                .map(CallOutput::Lenses),
            Call::OpenView {
                panel,
                contribution,
                args,
            } => done(self.run(call, |w, s| {
                w.arcode_plugin_guest().call_open_view(s, *panel, contribution, args)
            })?),
            Call::UiEvent { panel, event } => done(self.run(call, |w, s| {
                w.arcode_plugin_guest().call_handle_ui_event(s, *panel, event)
            })?),
            Call::FetchRows {
                panel,
                table,
                start,
                count,
            } => self
                .run(call, |w, s| {
                    w.arcode_plugin_guest().call_fetch_rows(s, *panel, *table, *start, *count)
                })?
                .map(CallOutput::Rows)
                .map_err(Failure::Guest),
            Call::ViewClosed(panel) => self
                .run(call, |w, s| w.arcode_plugin_guest().call_view_closed(s, *panel))
                .map(|()| CallOutput::Done),
        }
    }
}

impl From<Failure> for CallError {
    fn from(f: Failure) -> Self {
        match f {
            Failure::Trap { reason, .. } => Self::Trapped(reason),
            Failure::Guest(m) => Self::Plugin(m),
        }
    }
}

/// Converts a call outcome for the reply channel.
pub(crate) fn to_result(r: Result<CallOutput, Failure>) -> CallResult {
    r.map_err(CallError::from)
}
