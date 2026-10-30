//! Cancellation tokens and the blocking one-shot replies the plugin thread
//! waits on (quick picks, consent prompts). §5 "Cancellation".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How often a blocked plugin thread re-checks its cancel token while it
/// waits for the user. Only runs while a prompt is open, never when idle.
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// Set by the app (task chip, Esc, closing the panel); read by
/// `host.cancelled()`, blocking imports and the epoch callback.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Debug, Default)]
struct CancelInner {
    cancelled: AtomicBool,
    at: OnceLock<Instant>,
}

impl CancelToken {
    /// A fresh, uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Idempotent; the first call's time is kept so
    /// the hard-cancel grace period runs from it.
    pub fn cancel(&self) {
        let _ = self.inner.at.set(Instant::now());
        self.inner.cancelled.store(true, Ordering::SeqCst);
    }

    /// True once [`CancelToken::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Time since cancellation, if cancelled.
    pub fn cancelled_for(&self) -> Option<Duration> {
        self.inner.at.get().map(Instant::elapsed)
    }
}

/// Locks a mutex, ignoring poisoning: host state stays usable even if a
/// thread panicked while holding the lock (all guarded data is plain data).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Creates a one-shot reply channel.
pub fn oneshot<T>() -> (ReplySender<T>, ReplyReceiver<T>) {
    let slot = Arc::new(Slot {
        value: Mutex::new(SlotState::Empty),
        ready: Condvar::new(),
    });
    (ReplySender { slot: slot.clone() }, ReplyReceiver { slot })
}

#[derive(Debug)]
enum SlotState<T> {
    Empty,
    Full(T),
    /// The sender was dropped without answering.
    Dropped,
}

#[derive(Debug)]
struct Slot<T> {
    value: Mutex<SlotState<T>>,
    ready: Condvar,
}

/// The answering half (held by the app). Dropping it unanswered counts as
/// "dismissed".
#[derive(Debug)]
pub struct ReplySender<T> {
    slot: Arc<Slot<T>>,
}

impl<T> ReplySender<T> {
    /// Sends the answer and wakes the waiting plugin thread.
    pub fn send(self, value: T) {
        *lock(&self.slot.value) = SlotState::Full(value);
        self.slot.ready.notify_all();
    }
}

impl<T> Drop for ReplySender<T> {
    fn drop(&mut self) {
        let mut state = lock(&self.slot.value);
        if matches!(*state, SlotState::Empty) {
            *state = SlotState::Dropped;
            self.slot.ready.notify_all();
        }
    }
}

/// Why a wait ended without an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitError {
    /// The call's cancel token fired.
    Cancelled,
    /// The deadline passed.
    TimedOut,
    /// The sender was dropped unanswered.
    Dropped,
}

/// The waiting half (held by the plugin thread).
#[derive(Debug)]
pub struct ReplyReceiver<T> {
    slot: Arc<Slot<T>>,
}

impl<T> ReplyReceiver<T> {
    /// Blocks until the answer arrives, `cancel` fires or `timeout` passes.
    pub fn wait(self, cancel: &CancelToken, timeout: Option<Duration>) -> Result<T, WaitError> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut state = lock(&self.slot.value);
        loop {
            match std::mem::replace(&mut *state, SlotState::Empty) {
                SlotState::Full(v) => return Ok(v),
                SlotState::Dropped => return Err(WaitError::Dropped),
                SlotState::Empty => {}
            }
            if cancel.is_cancelled() {
                return Err(WaitError::Cancelled);
            }
            let mut step = CANCEL_POLL;
            if let Some(deadline) = deadline {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(WaitError::TimedOut);
                }
                step = step.min(left);
            }
            state = self
                .slot
                .ready
                .wait_timeout(state, step)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_arrives_across_threads() {
        let (tx, rx) = oneshot();
        std::thread::spawn(move || tx.send(7));
        assert_eq!(rx.wait(&CancelToken::new(), None), Ok(7));
    }

    #[test]
    fn dropped_sender_and_timeout_and_cancel() {
        let (tx, rx) = oneshot::<u8>();
        drop(tx);
        assert_eq!(rx.wait(&CancelToken::new(), None), Err(WaitError::Dropped));

        let (_tx, rx) = oneshot::<u8>();
        let timeout = Some(Duration::from_millis(20));
        assert_eq!(rx.wait(&CancelToken::new(), timeout), Err(WaitError::TimedOut));

        let (_tx, rx) = oneshot::<u8>();
        let token = CancelToken::new();
        let t2 = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            t2.cancel();
        });
        let start = Instant::now();
        assert_eq!(rx.wait(&token, None), Err(WaitError::Cancelled));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(token.cancelled_for().is_some());
    }
}
