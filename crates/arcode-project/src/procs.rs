//! Whether a terminal's shell is busy running a command, so closing the
//! terminal (closing or switching the project, closing the window,
//! quitting) can ask first.
//!
//! Best effort and blocking (it reads `/proc`, or runs `ps` or PowerShell):
//! call it off the UI thread. When it cannot tell, it answers `false`.

/// Whether the shell `shell_pid` is running a command in the foreground
/// (Unix: the terminal's foreground process group is not the shell's;
/// Windows: the shell has child processes).
pub fn is_running_command(shell_pid: u32) -> bool {
    imp::is_running_command(shell_pid).unwrap_or(false)
}

/// The process group and the terminal's foreground process group from a
/// Linux `/proc/<pid>/stat` line.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn groups_from_stat(stat: &str) -> Option<(i64, i64)> {
    // The command name (field 2) is in parentheses and may contain spaces
    // or parentheses; the fields after the last `)` are: state, ppid,
    // pgrp, session, tty_nr, tpgid, ...
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let pgrp = fields.get(2)?.parse().ok()?;
    let tpgid = fields.get(5)?.parse().ok()?;
    Some((pgrp, tpgid))
}

/// Whether a foreground group other than the shell's own holds the
/// terminal (`tpgid` is -1 or 0 without a controlling terminal).
#[cfg_attr(windows, allow(dead_code))]
fn foreign_foreground(pgrp: i64, tpgid: i64) -> bool {
    tpgid > 0 && tpgid != pgrp
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn is_running_command(pid: u32) -> Option<bool> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (pgrp, tpgid) = super::groups_from_stat(&stat)?;
        Some(super::foreign_foreground(pgrp, tpgid))
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod imp {
    use std::process::{Command, Stdio};

    pub fn is_running_command(pid: u32) -> Option<bool> {
        let out = Command::new("ps")
            .args(["-o", "pgid=,tpgid=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut fields = text.split_whitespace();
        let pgrp = fields.next()?.parse().ok()?;
        let tpgid = fields.next()?.parse().ok()?;
        Some(super::foreign_foreground(pgrp, tpgid))
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn is_running_command(pid: u32) -> Option<bool> {
        let script =
            format!("@(Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}').Count");
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let count: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some(count > 0)
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn is_running_command(_: u32) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_lines_are_parsed() {
        let stat = "4242 (my (odd) sh) S 1 4242 4242 34816 5000 4194560 0";
        assert_eq!(groups_from_stat(stat), Some((4242, 5000)));
        assert!(foreign_foreground(4242, 5000));
        assert!(!foreign_foreground(4242, 4242));
        assert!(!foreign_foreground(4242, -1));
        assert_eq!(groups_from_stat("garbage"), None);
    }

    #[test]
    fn a_process_without_a_terminal_is_not_busy() {
        // The test runner itself: no controlling terminal under CI, or the
        // runner owns the foreground; either way not a "foreign" group
        // unless run from a shell, which still must not panic.
        let _ = is_running_command(std::process::id());
        assert!(!is_running_command(u32::MAX));
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_running_a_foreground_command_is_busy() {
        use std::time::{Duration, Instant};
        // `script` gives the shell a terminal with job control; skip when
        // it is missing.
        let Ok(mut child) = std::process::Command::new("script")
            .args(["-qfec", "sh -ic 'sleep 30'", "/dev/null"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        // The `sh` started by `script` (its child).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut shell = None;
        let mut busy = false;
        while Instant::now() < deadline && !busy {
            std::thread::sleep(Duration::from_millis(100));
            shell = shell.or_else(|| shell_child(child.id()));
            busy = shell.is_some_and(is_running_command);
        }
        child.kill().ok();
        child.wait().ok();
        if let Some(shell) = shell {
            assert!(busy, "shell {shell} runs `sleep` in the foreground");
        }
    }

    #[cfg(unix)]
    fn shell_child(parent: u32) -> Option<u32> {
        let out = std::process::Command::new("pgrep")
            .args(["-P", &parent.to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .parse()
            .ok()
    }
}
