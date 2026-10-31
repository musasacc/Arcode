//! Builds and other jobs that can be stopped with everything they started.
//!
//! [`own_group`] puts a command's process in its own process group (Unix)
//! before it is spawned; [`kill_group`] then kills that group, so killing
//! `cargo` also stops the `rustc` processes it runs and killing `sh -c`
//! stops its command. On Windows the process tree is ended with
//! `taskkill /T` (started, not waited for).

use std::process::{Child, Command};

/// Makes `command`'s process the leader of a new process group (Unix; no
/// change elsewhere).
pub fn own_group(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// Kills `child` and the processes it started (see the module docs).
/// Does not wait for them.
pub fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: plain syscall; a negative pid names the process
            // group that `own_group` made `child` the leader of.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    let _ = child.kill();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Whether `pid` runs (a zombie waiting for a parent that never reaps
    /// it, as under some container inits, counts as stopped).
    fn alive(pid: i32) -> bool {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            let state = stat.rsplit(')').next().unwrap_or("").trim_start();
            return !state.starts_with('Z');
        }
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn killing_the_group_stops_grandchildren() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let mut child = own_group(Command::new("sh").args(["-c", &script]))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let grandchild: i32 = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse().ok())
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the script did not start");
            std::thread::sleep(Duration::from_millis(20));
        };
        kill_group(&mut child);
        let _ = child.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(grandchild) {
            assert!(Instant::now() < deadline, "the grandchild still runs");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
