//! CPU budgets with epochs (§5 "CPU budget with epochs").
//!
//! One [`Ticker`] thread per engine calls `increment_epoch` every
//! [`TICK`] **only while a call is in flight**; otherwise it parks on a
//! condvar. Each store's epoch callback runs only while wasm is executing
//! and charges one slice per invocation ([`SliceBudget::charge`]), so time
//! spent in host imports (prompts, HTTP, quick picks) is free.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

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
    if d.as_millis() >= 1000 && d.as_millis() % 1000 == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// Slices of guest execution charged against one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceBudget {
    budget: Duration,
    limit: u64,
    used: u64,
}

impl SliceBudget {
    /// A budget of `budget` worth of [`TICK`]s (at least one).
    pub fn new(budget: Duration) -> Self {
        let limit = (budget.as_nanos() / TICK.as_nanos()).max(1);
        Self {
            budget,
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
            used: 0,
        }
    }

    /// Raises the budget (`host.begin-task`); slices already used count.
    pub fn raise_to(&mut self, budget: Duration) {
        let used = self.used;
        *self = Self::new(budget.max(self.budget));
        self.used = used;
    }

    /// The budget.
    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// Slices charged so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Charges one slice; called from the epoch callback.
    pub fn charge(&mut self, cancel: &CancelToken, grace: Duration) -> Result<(), Interrupt> {
        self.used += 1;
        if cancel.cancelled_for().is_some_and(|t| t >= grace) {
            return Err(Interrupt::Cancelled);
        }
        if self.used > self.limit {
            return Err(Interrupt::Budget(self.budget));
        }
        Ok(())
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

fn run(inner: &TickerInner, engine: &wasmtime::Engine) {
    let mut state = lock(&inner.state);
    loop {
        while state.in_flight == 0 && !state.shutdown {
            state = inner.wake.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        if state.shutdown {
            return;
        }
        drop(state);
        std::thread::sleep(inner.period);
        engine.increment_epoch();
        inner.increments.fetch_add(1, Ordering::SeqCst);
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
        assert_eq!(Interrupt::Budget(Duration::from_secs(2)).to_string(), "exceeded 2 s CPU");
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
    fn hard_cancel_after_grace() {
        let token = CancelToken::new();
        let mut b = SliceBudget::new(Duration::from_secs(60));
        token.cancel();
        b.charge(&token, Duration::from_secs(5)).unwrap();
        assert_eq!(b.charge(&token, Duration::ZERO), Err(Interrupt::Cancelled));
    }
}
