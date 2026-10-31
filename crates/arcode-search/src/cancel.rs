//! Cooperative cancellation shared between the UI and a running search.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cheap, cloneable flag that stops a running search or scan promptly.
///
/// Clones share the same flag: keep one in the UI, pass `&token` to the
/// search, and call [`CancelToken::cancel`] when the query changes.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, not-yet-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Internal stop state: user cancellation plus "stop producing" (result
/// limit reached or the consumer asked to stop).
pub(crate) struct Halt<'a> {
    cancel: &'a CancelToken,
    stop: AtomicBool,
}

impl<'a> Halt<'a> {
    pub(crate) fn new(cancel: &'a CancelToken) -> Self {
        Self {
            cancel,
            stop: AtomicBool::new(false),
        }
    }

    /// Workers should stop producing results.
    pub(crate) fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.cancel.is_cancelled()
    }

    /// The user cancelled: pending results must be dropped.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Stop producing new results (already produced ones are still delivered).
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }

    #[test]
    fn halt_distinguishes_stop_and_cancel() {
        let token = CancelToken::new();
        let halt = Halt::new(&token);
        assert!(!halt.should_stop());
        halt.stop();
        assert!(halt.should_stop());
        assert!(!halt.is_cancelled());
        token.cancel();
        assert!(halt.is_cancelled());
    }
}
