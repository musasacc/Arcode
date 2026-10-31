//! Stopping a server together with the processes it started.
//!
//! Many servers are wrapper scripts or launchers (`sh -c`, `npx`, version
//! managers) whose real server is a grandchild. Killing only the direct
//! child leaves those running, so on Unix the server becomes the leader of
//! its own process group and [`GroupGuard`] kills the whole group when it
//! is dropped: after the server exits, when it is killed, and when its
//! supervisor task is dropped with the runtime. [`kill_all`] covers an app
//! that exits while servers still stop in the background, `signals.rs` an
//! app ended by a signal.

use std::sync::{Mutex, MutexGuard};

use tokio::process::Command;

/// Groups of servers that are still guarded, in the whole process.
static LIVE: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn live() -> MutexGuard<'static, Vec<i32>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Makes the server the leader of a new process group (Unix only).
pub(crate) fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Kills the process group led by a spawned server when dropped.
#[derive(Debug)]
pub(crate) struct GroupGuard {
    leader: Option<i32>,
    /// Its entry in the table the signal handler reads (`signals.rs`).
    slot: Option<usize>,
}

impl GroupGuard {
    /// Guards the group led by the process `pid` (from `Child::id`).
    pub(crate) fn new(pid: Option<u32>) -> Self {
        let leader = pid.and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 0);
        if let Some(pid) = leader {
            live().push(pid);
        }
        let slot = leader.and_then(super::signals::track);
        GroupGuard { leader, slot }
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.leader {
            // Removed first, so `kill_all` never signals a group id that
            // may since have been reused.
            live().retain(|p| *p != pid);
            if let Some(slot) = self.slot {
                super::signals::untrack(slot);
            }
            kill_group(pid);
        }
    }
}

/// Kills the process group of every language server this process started
/// that has not been stopped yet (in all managers and clients). Meant for
/// the moment the app exits, when graceful shutdowns still running in the
/// background would be cut off and leave the servers' children behind.
/// Does nothing on Windows, where servers are not grouped.
pub fn kill_all() {
    for pid in live().iter() {
        kill_group(*pid);
    }
}

fn kill_group(pid: i32) {
    #[cfg(unix)]
    // SAFETY: plain syscall; a negative pid names the group that
    // `own_group` made the server the leader of. Fails with ESRCH once
    // every member is gone.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Whether `pid` runs (a zombie counts as stopped).
    fn alive(pid: i32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z'),
            // SAFETY: signal 0 only checks that the process exists.
            Err(_) => unsafe { libc::kill(pid, 0) == 0 },
        }
    }

    /// Spawns a grouped `sh` that starts a long `sleep`; returns the shell,
    /// its guard and the sleep's pid.
    async fn wrapper_with_grandchild() -> (tokio::process::Child, GroupGuard, i32) {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &script]).kill_on_drop(true);
        own_group(&mut cmd);
        let child = cmd.spawn().unwrap();
        let guard = GroupGuard::new(child.id());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse().ok())
            {
                return (child, guard, pid);
            }
            assert!(Instant::now() < deadline, "the script did not start");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn assert_stops(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(pid) {
            assert!(Instant::now() < deadline, "the grandchild still runs");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn dropping_the_guard_stops_grandchildren() {
        let (mut child, guard, grandchild) = wrapper_with_grandchild().await;
        let leader = guard.leader.unwrap();
        assert!(live().contains(&leader));
        drop(guard);
        assert!(!live().contains(&leader), "a dropped guard stays listed");
        let _ = child.wait().await;
        assert_stops(grandchild).await;
    }

    #[tokio::test]
    async fn kill_all_stops_every_guarded_group() {
        let (mut child, guard, grandchild) = wrapper_with_grandchild().await;
        kill_all();
        let _ = child.wait().await;
        assert_stops(grandchild).await;
        drop(guard);
    }
}
