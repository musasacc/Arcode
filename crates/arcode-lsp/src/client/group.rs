//! Stopping a server together with the processes it started.
//!
//! Many servers are wrapper scripts or launchers (`sh -c`, `npx`, version
//! managers) whose real server is a grandchild. Killing only the direct
//! child leaves those running, so on Unix the server becomes the leader of
//! its own process group and [`GroupGuard`] kills the whole group when it
//! is dropped: after the server exits, when it is killed, and when its
//! supervisor task is dropped with the runtime.

use tokio::process::Command;

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
    #[cfg_attr(not(unix), allow(dead_code))]
    leader: Option<i32>,
}

impl GroupGuard {
    /// Guards the group led by the process `pid` (from `Child::id`).
    pub(crate) fn new(pid: Option<u32>) -> Self {
        GroupGuard {
            leader: pid.and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 0),
        }
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.leader {
            // SAFETY: plain syscall; a negative pid names the group that
            // `own_group` made the server the leader of. Fails with ESRCH
            // once every member is gone.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
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

    #[tokio::test]
    async fn dropping_the_guard_stops_grandchildren() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &script]).kill_on_drop(true);
        own_group(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let guard = GroupGuard::new(child.id());
        let deadline = Instant::now() + Duration::from_secs(5);
        let grandchild: i32 = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse().ok())
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the script did not start");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        drop(guard);
        let _ = child.wait().await;
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(grandchild) {
            assert!(Instant::now() < deadline, "the grandchild still runs");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
