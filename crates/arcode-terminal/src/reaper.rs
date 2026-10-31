//! The I/O thread: runs alacritty's event loop and, once it ends, tears
//! the PTY down on that thread so the caller (the UI) never blocks.
//!
//! alacritty's `EventLoop::spawn` hands the loop, which owns the PTY, back
//! through its `JoinHandle`; dropping that PTY hangs up the child and then
//! waits for it. We join the loop on our own thread and drop the PTY there,
//! right after the loop ends. On Unix a child still running at that point
//! gets `SIGHUP` and, if it survives [`KILL_GRACE`], `SIGKILL`. The
//! processes it started in its own process group get `SIGHUP` too.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use alacritty_terminal::event_loop::EventLoop;
use alacritty_terminal::tty::Pty;

use crate::events::{EventProxy, Shared};

/// How long a hung-up child may take to exit before it is killed.
pub const KILL_GRACE: Duration = Duration::from_secs(1);

/// Runs `event_loop` and cleans up after it, all off the caller's thread.
/// The returned handle finishes once the child is gone and the PTY closed.
pub(crate) fn spawn_io_thread(
    event_loop: EventLoop<Pty, EventProxy>,
    shared: Arc<Shared>,
    child_pid: Option<u32>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("arcode-terminal-io".into())
        .spawn(move || {
            let Ok((event_loop, _state)) = event_loop.spawn().join() else {
                log::error!("terminal: the PTY event loop panicked");
                shared.alive.store(false, Ordering::SeqCst);
                return;
            };
            shared.alive.store(false, Ordering::SeqCst);
            // A reaped child's pid may already belong to another process.
            if !shared.child_reaped.load(Ordering::SeqCst)
                && let Some(pid) = child_pid
            {
                terminate(pid, KILL_GRACE);
            }
            // Closes the PTY; on Unix this also reaps the child.
            drop(event_loop);
        })
}

/// Hangs up the (not yet reaped) child, and what it started in its process
/// group, and kills the child if it is still running after `grace`. Its
/// pid stays reserved until the PTY reaps it.
#[cfg(unix)]
pub(crate) fn terminate(pid: u32, grace: Duration) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    if unix::has_exited(pid) {
        return;
    }
    crate::hangup::hang_up(pid);
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if unix::has_exited(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !unix::has_exited(pid) {
        log::debug!("terminal: child {pid} ignored SIGHUP, killing it");
        unix::signal(pid, libc::SIGKILL);
    }
}

/// On Windows, closing the pseudo console (dropping the PTY) terminates
/// the processes attached to it.
#[cfg(not(unix))]
pub(crate) fn terminate(_pid: u32, _grace: Duration) {}

#[cfg(unix)]
mod unix {
    /// Sends `sig` to `pid`.
    pub(super) fn signal(pid: libc::pid_t, sig: libc::c_int) {
        // SAFETY: plain syscall; the caller guarantees `pid` is an unreaped
        // child of this process, so it cannot name an unrelated process.
        unsafe {
            libc::kill(pid, sig);
        }
    }

    /// Whether the child `pid` has terminated, without reaping it (so the
    /// PTY can still wait for it). A pid that is not our child counts as
    /// exited.
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    pub(super) fn has_exited(pid: libc::pid_t) -> bool {
        // SAFETY: `info` is a zeroed, writable siginfo_t; with WNOHANG the
        // call never blocks and with WNOWAIT the child is left waitable.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            let id = pid as libc::id_t;
            let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
            if libc::waitid(libc::P_PID, id, &mut info, flags) != 0 {
                // ECHILD: not our child (anymore); EINTR: check again later.
                return std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR);
            }
            // With WNOHANG and nothing to report, si_pid stays zero.
            info.si_pid() != 0
        }
    }

    /// Fallback without `waitid(WNOWAIT)`: assume it runs; signalling an
    /// unreaped zombie is harmless.
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    pub(super) fn has_exited(_pid: libc::pid_t) -> bool {
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    fn spawn(script: &str) -> std::process::Child {
        Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn sh")
    }

    #[test]
    fn exited_child_is_left_for_the_owner_to_reap() {
        let mut child = spawn("exit 7");
        let pid = child.id() as libc::pid_t;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !unix::has_exited(pid) {
            assert!(Instant::now() < deadline, "child never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        // Still waitable: has_exited must not reap it.
        assert_eq!(child.wait().expect("wait").code(), Some(7));
        // Reaped now: no longer our child, counts as exited.
        assert!(unix::has_exited(pid));
    }

    #[test]
    fn terminate_hangs_up_a_running_child() {
        let mut child = spawn("sleep 30");
        let started = Instant::now();
        terminate(child.id(), Duration::from_secs(5));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "SIGHUP was enough"
        );
        assert!(child.wait().expect("wait").code().is_none());
    }

    #[test]
    fn terminate_kills_a_child_that_ignores_sighup() {
        let mut child = spawn("trap '' HUP; while :; do sleep 0.05; done");
        // Let the shell install its trap first.
        std::thread::sleep(Duration::from_millis(200));
        assert!(!unix::has_exited(child.id() as libc::pid_t));
        terminate(child.id(), Duration::from_millis(300));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                assert!(!status.success());
                break;
            }
            assert!(Instant::now() < deadline, "child survived SIGKILL");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
