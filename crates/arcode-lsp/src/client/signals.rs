//! Kills every server's process group when a signal ends the app (Ctrl+C
//! or a hangup of the terminal it was started from, `kill`).
//!
//! Servers lead process groups of their own (see `group.rs`), so such a
//! signal no longer reaches them and their helpers through the terminal's
//! foreground group, and no quit callback runs either. [`install`] adds a
//! handler that kills the groups and then lets the signal end the app as
//! it would have. A signal handler may only touch atomics, so the guarded
//! groups are mirrored into a fixed table here.

use std::sync::atomic::{AtomicI32, Ordering};

/// Far more servers than an app runs at once; a group that finds no free
/// slot is still killed on quit and on drop, only not on a signal.
const SLOTS: usize = 64;

/// Leaders of the guarded groups; 0 marks a free slot.
static GROUPS: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];

/// Lists the group led by `pid`; returns its slot, if one was free.
pub(super) fn track(pid: i32) -> Option<usize> {
    GROUPS.iter().position(|slot| {
        slot.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    })
}

/// Frees a slot [`track`] returned.
pub(super) fn untrack(slot: usize) {
    GROUPS[slot].store(0, Ordering::Release);
}

/// Makes `SIGHUP`, `SIGINT`, `SIGQUIT` and `SIGTERM` kill every server's
/// process group before they end the app. Signals the app was started
/// with a disposition other than the default for (`nohup` ignores
/// `SIGHUP`) are left alone. Call once, early; later calls do nothing.
/// Does nothing on Windows, where servers are not grouped.
pub fn install() {
    #[cfg(unix)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            for sig in [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM] {
                unix::install(sig);
            }
        });
    }
}

#[cfg(unix)]
mod unix {
    use super::{GROUPS, Ordering};

    pub(super) fn install(sig: libc::c_int) {
        // SAFETY: both `sigaction` structs are zeroed and then filled in;
        // the handler only makes async-signal-safe calls.
        unsafe {
            let mut old: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(sig, std::ptr::null(), &mut old) != 0
                || old.sa_sigaction != libc::SIG_DFL
            {
                return;
            }
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            // The default action is back as soon as the handler runs.
            action.sa_flags = libc::SA_RESETHAND;
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }

    extern "C" fn on_signal(sig: libc::c_int) {
        for slot in &GROUPS {
            let pid = slot.load(Ordering::Acquire);
            if pid > 0 {
                // SAFETY: `kill` is async-signal-safe; a negative pid names
                // the group the server leads.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
        // SAFETY: `raise` is async-signal-safe. The signal is blocked while
        // its handler runs, so it is delivered, with its default action,
        // right after this returns: the app ends as it would have.
        unsafe {
            libc::raise(sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_taken_and_freed() {
        // Leader ids no real group has, so no other test is disturbed.
        let a = track(-7).expect("a free slot");
        let b = track(-8).expect("another free slot");
        assert_ne!(a, b);
        assert_eq!(GROUPS[a].load(Ordering::Acquire), -7);
        untrack(a);
        assert_eq!(GROUPS[a].load(Ordering::Acquire), 0);
        untrack(b);
    }
}
