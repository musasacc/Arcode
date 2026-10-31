//! Two WASI functions the host replaces on top of wasmtime-wasi's sync
//! linker (§4 "WASI context", §5 "Cancellation"):
//!
//! - `wasi:clocks/monotonic-clock` `subscribe-duration`/`subscribe-instant`
//!   return pollables that also wake when the current call is cancelled or
//!   its budget runs out ([`WaitGate`]). A guest blocked in a WASI sleep is
//!   otherwise unreachable: the epoch callback only runs while wasm runs.
//!   The sleep is charged to the budget by the call hook when it returns.
//! - `wasi:filesystem/types` `[method]descriptor.open-at` refuses to open a
//!   file (with `insufficient-memory`, as wasi-libc has no EMFILE) once the
//!   store's resource table is nearly full, instead of trapping. Every open
//!   file is a host descriptor; `StoreCaps::resources` bounds them.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasmtime::component::{HasSelf, Linker, Resource, ResourceTable};
use wasmtime_wasi::filesystem::WasiFilesystemView;
use wasmtime_wasi::p2::bindings::clocks::monotonic_clock;
use wasmtime_wasi::p2::bindings::sync::filesystem::types::{
    self as fs, Descriptor, DescriptorFlags, ErrorCode, OpenFlags, PathFlags,
};
use wasmtime_wasi::p2::{DynPollable, Pollable, subscribe};

use crate::cancel::{CancelToken, lock};
use crate::state::HostState;

/// How often a sleeping guest's pollable re-checks the gate.
const GATE_POLL: Duration = Duration::from_millis(10);
/// Table entries kept free for streams and pollables when opening files.
const OPEN_MARGIN: usize = 16;
/// The WASI version wasmtime-wasi 50 links (guests match it by semver).
const FS_TYPES: &str = "wasi:filesystem/types@0.2.12";

/// What the current call allows a WASI wait: the call's cancel token and
/// the instant its budget would run out. Re-armed by the call hook on every
/// entry into the host, so a pollable created in an earlier call obeys the
/// current one.
#[derive(Debug, Clone, Default)]
pub struct WaitGate(Arc<Mutex<Gate>>);

#[derive(Debug, Default)]
struct Gate {
    cancel: CancelToken,
    wake_by: Option<Instant>,
}

impl WaitGate {
    /// Sets the current call's token and budget end.
    pub fn arm(&self, cancel: &CancelToken, wake_by: Instant) {
        let mut gate = lock(&self.0);
        gate.cancel = cancel.clone();
        gate.wake_by = Some(wake_by);
    }

    /// True if a wait must end now.
    fn open(&self, now: Instant) -> bool {
        let gate = lock(&self.0);
        gate.cancel.is_cancelled() || gate.wake_by.is_some_and(|t| now >= t)
    }
}

/// A monotonic-clock deadline that also opens with the [`WaitGate`].
struct GatedDeadline {
    /// `None`: never (a duration too large to represent).
    until: Option<Instant>,
    gate: WaitGate,
}

#[wasmtime_wasi::async_trait]
impl Pollable for GatedDeadline {
    async fn ready(&mut self) {
        loop {
            let now = Instant::now();
            if self.until.is_some_and(|u| now >= u) || self.gate.open(now) {
                return;
            }
            let step = self.until.map_or(GATE_POLL, |u| {
                u.saturating_duration_since(now).min(GATE_POLL)
            });
            tokio::time::sleep(step).await;
        }
    }
}

impl HostState {
    fn subscribe_gated(&mut self, after: Duration) -> wasmtime::Result<Resource<DynPollable>> {
        let deadline = GatedDeadline {
            until: Instant::now().checked_add(after),
            gate: self.gate.clone(),
        };
        let resource = self.table.push(deadline)?;
        subscribe(&mut self.table, resource)
    }
}

impl monotonic_clock::Host for HostState {
    fn now(&mut self) -> wasmtime::Result<monotonic_clock::Instant> {
        monotonic_clock::Host::now(&mut wasmtime_wasi::clocks::WasiClocksView::clocks(self))
    }

    fn resolution(&mut self) -> wasmtime::Result<monotonic_clock::Duration> {
        monotonic_clock::Host::resolution(&mut wasmtime_wasi::clocks::WasiClocksView::clocks(self))
    }

    fn subscribe_instant(
        &mut self,
        when: monotonic_clock::Instant,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        let now = monotonic_clock::Host::now(self)?;
        self.subscribe_gated(Duration::from_nanos(when.saturating_sub(now)))
    }

    fn subscribe_duration(
        &mut self,
        duration: monotonic_clock::Duration,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        self.subscribe_gated(Duration::from_nanos(duration))
    }
}

/// True if `table` can take `margin` more entries.
fn has_room(table: &mut ResourceTable, margin: usize) -> bool {
    let cap = table.max_capacity();
    table.set_max_capacity(cap.saturating_sub(margin));
    let room = match table.push(()) {
        Ok(probe) => table.delete(probe).is_ok(),
        Err(_) => false,
    };
    table.set_max_capacity(cap);
    room
}

type OpenAtParams = (
    Resource<Descriptor>,
    PathFlags,
    String,
    OpenFlags,
    DescriptorFlags,
);
type OpenAtResult = (Result<Resource<Descriptor>, ErrorCode>,);

fn open_at(state: &mut HostState, params: OpenAtParams) -> wasmtime::Result<OpenAtResult> {
    if !has_room(&mut state.table, OPEN_MARGIN) {
        return Ok((Err(ErrorCode::InsufficientMemory),));
    }
    let (fd, path_flags, path, oflags, flags) = params;
    let mut view = state.filesystem();
    match fs::HostDescriptor::open_at(&mut view, fd, path_flags, path, oflags, flags) {
        Ok(opened) => Ok((Ok(opened),)),
        Err(e) => Ok((Err(fs::Host::convert_error_code(&mut view, e)?),)),
    }
}

/// Replaces the two WASI functions above; call after
/// `wasmtime_wasi::p2::add_to_linker_sync`.
pub fn add_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    linker.allow_shadowing(true);
    monotonic_clock::add_to_linker::<HostState, HasSelf<HostState>>(linker, |s| s)?;
    linker
        .instance(FS_TYPES)?
        .func_wrap("[method]descriptor.open-at", |mut store, params| {
            open_at(store.data_mut(), params)
        })?;
    linker.allow_shadowing(false);
    Ok(())
}
