//! Memory limits and crash backoff (§5 "Memory", "Crashes").

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use wasmtime::{ResourceLimiter, StoreLimits, StoreLimitsBuilder};

/// Per-store resource caps besides linear memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreCaps {
    /// Linear memories per store.
    pub memories: usize,
    /// Tables per store.
    pub tables: usize,
    /// Core instances per store (a component instantiates several).
    pub instances: usize,
    /// Elements per table.
    pub table_elements: usize,
}

impl Default for StoreCaps {
    /// Measured in step 3 by lowering each cap until instantiation failed:
    /// the Rust `wasm32-wasip2` fixtures (echo, spin, fs-escape) need 1
    /// memory, 2 tables and 3 core instances. The §5 starting values
    /// (2, 8, 16) are kept as headroom for C code such as SQLite.
    fn default() -> Self {
        Self {
            memories: 2,
            tables: 8,
            instances: 16,
            table_elements: 100_000,
        }
    }
}

/// [`StoreLimits`] that remembers whether it refused a memory growth, so a
/// following trap can be reported as "out of memory".
#[derive(Debug)]
pub struct Limiter {
    inner: StoreLimits,
    memory_denied: bool,
    peak_memory: usize,
}

impl Limiter {
    /// Caps linear memory at `memory_bytes`. Growth beyond it fails inside
    /// the guest (`trap_on_grow_failure(false)`), so the guest's allocator
    /// sees the failure in its own sandbox.
    pub fn new(memory_bytes: u64, caps: &StoreCaps) -> Self {
        let inner = StoreLimitsBuilder::new()
            .memory_size(usize::try_from(memory_bytes).unwrap_or(usize::MAX))
            .memories(caps.memories)
            .tables(caps.tables)
            .instances(caps.instances)
            .table_elements(caps.table_elements)
            .trap_on_grow_failure(false)
            .build();
        Self {
            inner,
            memory_denied: false,
            peak_memory: 0,
        }
    }

    /// True once a memory growth was refused.
    pub fn memory_denied(&self) -> bool {
        self.memory_denied
    }

    /// Largest linear memory size granted so far, in bytes.
    pub fn peak_memory(&self) -> usize {
        self.peak_memory
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let ok = self.inner.memory_growing(current, desired, maximum)?;
        if ok {
            self.peak_memory = self.peak_memory.max(desired);
        } else {
            self.memory_denied = true;
        }
        Ok(ok)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.inner.table_growing(current, desired, maximum)
    }

    fn instances(&self) -> usize {
        self.inner.instances()
    }

    fn tables(&self) -> usize {
        self.inner.tables()
    }

    fn memories(&self) -> usize {
        self.inner.memories()
    }
}

/// When to restart after traps (§5: three traps in ten minutes disable the
/// plugin for the session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashPolicy {
    /// Traps within `window` that disable the plugin.
    pub max_crashes: usize,
    /// The sliding window.
    pub window: Duration,
    /// Wait before the restart after the second trap; doubles after that.
    pub backoff: Duration,
}

impl Default for CrashPolicy {
    fn default() -> Self {
        Self {
            max_crashes: 3,
            window: Duration::from_secs(600),
            backoff: Duration::from_secs(1),
        }
    }
}

/// What to do after a trap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashVerdict {
    /// Re-instantiate on the next event, not before `not_before`.
    Restart {
        /// Earliest restart.
        not_before: Instant,
    },
    /// Disabled for the session.
    Disable,
}

/// Recent traps of one plugin.
#[derive(Debug, Clone, Default)]
pub struct CrashTracker {
    times: VecDeque<Instant>,
    disabled: bool,
    not_before: Option<Instant>,
}

impl CrashTracker {
    /// Records a trap at `now`.
    pub fn record(&mut self, now: Instant, policy: &CrashPolicy) -> CrashVerdict {
        while self
            .times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > policy.window)
        {
            self.times.pop_front();
        }
        self.times.push_back(now);
        let n = self.times.len();
        if n >= policy.max_crashes.max(1) {
            self.disabled = true;
            return CrashVerdict::Disable;
        }
        // First trap: restart right away. Then 1×, 2×, 4× the backoff.
        let wait = match n {
            0 | 1 => Duration::ZERO,
            n => policy.backoff.saturating_mul(1 << (n - 2).min(16) as u32),
        };
        let not_before = now + wait;
        self.not_before = Some(not_before);
        CrashVerdict::Restart { not_before }
    }

    /// True once disabled for the session.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Remaining backoff at `now`, if any.
    pub fn backoff_left(&self, now: Instant) -> Option<Duration> {
        let left = self.not_before?.saturating_duration_since(now);
        (!left.is_zero()).then_some(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_traps_in_the_window_disable() {
        let policy = CrashPolicy::default();
        let mut t = CrashTracker::default();
        let t0 = Instant::now();
        assert_eq!(
            t.record(t0, &policy),
            CrashVerdict::Restart { not_before: t0 }
        );
        assert_eq!(t.backoff_left(t0), None);
        let t1 = t0 + Duration::from_secs(5);
        assert_eq!(
            t.record(t1, &policy),
            CrashVerdict::Restart {
                not_before: t1 + Duration::from_secs(1)
            }
        );
        assert_eq!(t.backoff_left(t1), Some(Duration::from_secs(1)));
        assert_eq!(t.backoff_left(t1 + Duration::from_secs(2)), None);
        assert_eq!(
            t.record(t1 + Duration::from_secs(9), &policy),
            CrashVerdict::Disable
        );
        assert!(t.is_disabled());
    }

    #[test]
    fn old_traps_fall_out_of_the_window() {
        let policy = CrashPolicy::default();
        let mut t = CrashTracker::default();
        let t0 = Instant::now();
        t.record(t0, &policy);
        t.record(t0 + Duration::from_secs(1), &policy);
        let later = t0 + Duration::from_secs(700);
        assert!(matches!(
            t.record(later, &policy),
            CrashVerdict::Restart { .. }
        ));
        assert!(!t.is_disabled());
    }

    #[test]
    fn limiter_records_denied_growth() {
        let mut l = Limiter::new(2 * 65536, &StoreCaps::default());
        assert!(l.memory_growing(0, 65536, None).unwrap());
        assert!(!l.memory_growing(65536, 3 * 65536, None).unwrap());
        assert!(l.memory_denied());
        assert_eq!(l.peak_memory(), 65536);
        assert_eq!((l.memories(), l.tables(), l.instances()), (2, 8, 16));
    }
}
