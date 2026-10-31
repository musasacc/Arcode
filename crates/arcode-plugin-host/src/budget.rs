//! CPU budgets with epochs (§5 "CPU budget with epochs").
//!
//! One [`Ticker`] thread per engine calls `increment_epoch` every
//! [`TICK`] **only while a call is in flight**; otherwise it parks on a
//! condvar. Each store's epoch callback runs only while wasm is executing
//! and charges one slice per invocation ([`SliceBudget::charge`]).
//!
//! Host imports are charged by a store call hook ([`SliceBudget::charge_host`]):
//! the time an import took, minus the time it spent waiting for the user or
//! the network (prompts, quick picks, `http.send`, editor round trips), so a
//! guest cannot burn host CPU for free by looping over an expensive import,
//! and a WASI sleep counts like guest time. The hard-cancel grace runs from
//! the later of the cancel and the start of the call, so a call cancelled
//! while still queued gets its full grace once it runs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::cancel::{CancelToken, lock};

/// The epoch period.
pub const TICK: Duration = Duration::from_millis(10);

/// Guest CPU budgets per export (§5 table) and the hard-cancel grace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallBudgets {
    /// `activate`.
    pub activate: Duration,
    /// `deactivate` (idle unload).
    pub deactivate: Duration,
    /// `provide-code-lenses`.
    pub code_lenses: Duration,
    /// `open-view`, `handle-ui-event`, `fetch-rows`, `view-closed`.
    pub view: Duration,
    /// `handle-command`.
    pub command: Duration,
    /// After cancel, how long cooperative cancellation gets before the
    /// epoch callback traps.
    pub cancel_grace: Duration,
}

impl Default for CallBudgets {
    fn default() -> Self {
        Self {
            activate: Duration::from_secs(1),
            deactivate: Duration::from_millis(100),
            code_lenses: Duration::from_millis(100),
            view: Duration::from_secs(1),
            command: Duration::from_secs(2),
            cancel_grace: Duration::from_secs(2),
        }
    }
}

/// Why the epoch callback stopped a call. Carried inside the trap error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// The guest used up its CPU budget.
    Budget(Duration),
    /// Cancelled and did not stop within the grace period.
    Cancelled,
}

impl std::fmt::Display for Interrupt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget(d) => write!(f, "exceeded {} CPU", human(*d)),
            Self::Cancelled => f.write_str("did not stop after cancel"),
        }
    }
}

impl std::error::Error for Interrupt {}

/// "2 s", "100 ms".
pub fn human(d: Duration) -> String {
    if d.as_millis() >= 1000 && d.as_millis().is_multiple_of(1000) {
        format!("{} s", d.as_secs())
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// Slices of guest execution (plus charged host time) against one call.
///
/// Guest time is the larger of the epoch slices seen and the wall time
/// the guest ran (measured at each epoch callback, minus host imports):
/// a ticker that falls behind (timer coalescing on a loaded macOS
/// machine) or several epochs that pass while the guest thread is
/// descheduled (one callback for all of them) would otherwise let a
/// guest run well past its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceBudget {
    budget: Duration,
    limit: u64,
    used: u64,
    host: Duration,
    started: Instant,
    /// Wall time the guest ran up to `resumed`.
    guest: Duration,
    /// When the guest last started running (`None` inside a host import).
    resumed: Option<Instant>,
}

impl SliceBudget {
    /// A budget of `budget` worth of [`TICK`]s (at least one).
    pub fn new(budget: Duration) -> Self {
        let limit = (budget.as_nanos() / TICK.as_nanos()).max(1);
        let now = Instant::now();
        Self {
            budget,
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
            used: 0,
            host: Duration::ZERO,
            started: now,
            guest: Duration::ZERO,
            resumed: Some(now),
        }
    }

    /// Raises the budget (`host.begin-task`); what was used still counts.
    pub fn raise_to(&mut self, budget: Duration) {
        let kept = *self;
        *self = Self::new(budget.max(self.budget));
        (self.used, self.host, self.started) = (kept.used, kept.host, kept.started);
        (self.guest, self.resumed) = (kept.guest, kept.resumed);
    }

    /// Slices used: guest slices (at least the guest's wall time) plus
    /// charged host time, in whole ticks.
    fn slices(&self) -> u64 {
        let ticks = |d: Duration| u64::try_from(d.as_nanos() / TICK.as_nanos()).unwrap_or(u64::MAX);
        self.used
            .max(ticks(self.guest))
            .saturating_add(ticks(self.host))
    }

    /// Adds the guest's run time up to `now` (and restarts it from `now`).
    fn clock_guest(&mut self, now: Instant) {
        if let Some(since) = self.resumed {
            self.guest = self
                .guest
                .saturating_add(now.saturating_duration_since(since));
            self.resumed = Some(now);
        }
    }

    /// The guest calls into the host: its clock stops (the import's time
    /// is charged by [`SliceBudget::charge_host`]).
    pub fn pause(&mut self) {
        self.clock_guest(Instant::now());
        self.resumed = None;
    }

    /// The guest runs again after a host import.
    pub fn resume(&mut self) {
        self.resumed = Some(Instant::now());
    }

    /// Budget left (zero once used up).
    pub fn remaining(&self) -> Duration {
        let left = self.limit.saturating_sub(self.slices());
        TICK.saturating_mul(u32::try_from(left).unwrap_or(u32::MAX))
    }

    /// Charges `spent` of host time (call hook, on return to the guest).
    pub fn charge_host(
        &mut self,
        spent: Duration,
        cancel: &CancelToken,
        grace: Duration,
    ) -> Result<(), Interrupt> {
        self.host = self.host.saturating_add(spent);
        self.check(cancel, grace)
    }

    /// True once the call was cancelled at least `grace` ago, counting
    /// from the start of the call if it was cancelled before it started.
    fn hard_cancel(&self, cancel: &CancelToken, grace: Duration) -> bool {
        cancel
            .cancelled_at()
            .is_some_and(|at| at.max(self.started).elapsed() >= grace)
    }

    fn check(&self, cancel: &CancelToken, grace: Duration) -> Result<(), Interrupt> {
        if self.hard_cancel(cancel, grace) {
            return Err(Interrupt::Cancelled);
        }
        if self.slices() > self.limit {
            return Err(Interrupt::Budget(self.budget));
        }
        Ok(())
    }

    /// The budget.
    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// Slices charged so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Charges one slice (and the guest's run time); called from the
    /// epoch callback.
    pub fn charge(&mut self, cancel: &CancelToken, grace: Duration) -> Result<(), Interrupt> {
        self.used += 1;
        self.clock_guest(Instant::now());
        self.check(cancel, grace)
    }
}

#[derive(Debug, Default)]
struct TickerState {
    in_flight: usize,
    shutdown: bool,
}

#[derive(Debug)]
struct TickerInner {
    state: Mutex<TickerState>,
    wake: Condvar,
    increments: AtomicU64,
    period: Duration,
}

/// The shared epoch ticker of one engine.
#[derive(Debug)]
pub struct Ticker {
    inner: Arc<TickerInner>,
}

impl Ticker {
    /// Starts the (parked) ticker thread for `engine`.
    pub fn start(engine: wasmtime::Engine, period: Duration) -> std::io::Result<Self> {
        let inner = Arc::new(TickerInner {
            state: Mutex::new(TickerState::default()),
            wake: Condvar::new(),
            increments: AtomicU64::new(0),
            period,
        });
        let thread_inner = inner.clone();
        std::thread::Builder::new()
            .name("plugin-epoch".into())
            .stack_size(64 * 1024)
            .spawn(move || run(&thread_inner, &engine))?;
        Ok(Self { inner })
    }

    /// Marks a call in flight until the guard drops.
    pub fn enter(&self) -> InFlight {
        lock(&self.inner.state).in_flight += 1;
        self.inner.wake.notify_all();
        InFlight {
            inner: self.inner.clone(),
        }
    }

    /// Epoch increments so far (idle-cost counter).
    pub fn increments(&self) -> u64 {
        self.inner.increments.load(Ordering::SeqCst)
    }

    /// Calls currently in flight.
    pub fn in_flight(&self) -> usize {
        lock(&self.inner.state).in_flight
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        lock(&self.inner.state).shutdown = true;
        self.inner.wake.notify_all();
    }
}

/// Ticks on a fixed schedule (no drift from sleep overshoot), so N slices
/// are close to N periods of wall time while the guest runs.
fn run(inner: &TickerInner, engine: &wasmtime::Engine) {
    let mut state = lock(&inner.state);
    let mut next: Option<Instant> = None;
    loop {
        if state.in_flight == 0 {
            next = None;
        }
        while state.in_flight == 0 && !state.shutdown {
            state = inner
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if state.shutdown {
            return;
        }
        drop(state);
        let now = Instant::now();
        let due = match next {
            // Fell behind (e.g. the machine was suspended): restart the schedule.
            Some(due) if due + inner.period > now => due,
            _ => now + inner.period,
        };
        std::thread::sleep(due.saturating_duration_since(now));
        engine.increment_epoch();
        inner.increments.fetch_add(1, Ordering::SeqCst);
        next = Some(due + inner.period);
        state = lock(&inner.state);
    }
}

/// Keeps the ticker running while alive.
#[derive(Debug)]
pub struct InFlight {
    inner: Arc<TickerInner>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        state.in_flight = state.in_flight.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_trap_after_the_budget() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_millis(100));
        for _ in 0..10 {
            b.charge(&token, Duration::from_secs(2)).unwrap();
        }
        assert_eq!(
            b.charge(&token, Duration::from_secs(2)),
            Err(Interrupt::Budget(Duration::from_millis(100)))
        );
        assert_eq!(
            Interrupt::Budget(Duration::from_secs(2)).to_string(),
            "exceeded 2 s CPU"
        );
    }

    #[test]
    fn raising_keeps_used_slices_and_never_lowers() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_millis(20));
        b.charge(&token, Duration::ZERO).unwrap();
        b.raise_to(Duration::from_secs(1));
        assert_eq!((b.used(), b.budget()), (1, Duration::from_secs(1)));
        b.raise_to(Duration::from_millis(10));
        assert_eq!(b.budget(), Duration::from_secs(1));
    }

    #[test]
    fn host_time_counts_and_grace_starts_with_the_call() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_millis(100));
        b.charge_host(Duration::from_millis(95), &token, Duration::ZERO)
            .unwrap();
        assert_eq!(b.remaining(), Duration::from_millis(10));
        assert_eq!(
            b.charge_host(Duration::from_millis(20), &token, Duration::ZERO),
            Err(Interrupt::Budget(Duration::from_millis(100)))
        );
        // Cancelled long before this call started: the grace starts now.
        token.cancel();
        std::thread::sleep(Duration::from_millis(30));
        let mut b = SliceBudget::new(Duration::from_secs(60));
        assert_eq!(b.charge(&token, Duration::from_millis(20)), Ok(()));
    }

    /// Ticks lost by a late ticker, or epochs that passed while the guest
    /// was descheduled (one callback), still count: the wall time the
    /// guest ran does.
    #[test]
    fn guest_run_time_counts_when_ticks_are_lost() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            b.charge(&token, Duration::from_secs(2)),
            Err(Interrupt::Budget(Duration::from_millis(30)))
        );
        // Time inside a host import is not guest time (charged apart).
        let mut b = SliceBudget::new(Duration::from_millis(30));
        b.pause();
        std::thread::sleep(Duration::from_millis(50));
        b.resume();
        assert_eq!(b.charge(&token, Duration::from_secs(2)), Ok(()));
    }

    #[test]
    fn hard_cancel_after_grace() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_secs(60));
        token.cancel();
        b.charge(&token, Duration::from_secs(5)).unwrap();
        assert_eq!(b.charge(&token, Duration::ZERO), Err(Interrupt::Cancelled));
    }
}
