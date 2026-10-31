//! Hangs up a terminal's child together with what it started.
//!
//! A shell that is not running a job in the foreground (`sh -c 'sleep 30'`
//! under dash, a script) forks its commands into its own process group.
//! Hanging up only the shell orphans them, where a real terminal hangup
//! would have reached the whole group. Jobs an interactive shell moved
//! into groups of their own are left to the shell, as a real hangup does.

/// Sends `SIGHUP` to `pid` and, on Linux, to its descendants that are still
/// in its process group.
///
/// The child and those descendants are stopped first, so none of them can
/// fork a process this would miss (the kernel aborts a `fork` that races a
/// signal), and continued afterwards so they act on the hangup.
#[cfg(target_os = "linux")]
pub(crate) fn hang_up(pid: i32) {
    signal(pid, libc::SIGSTOP);
    let mut stopped: Vec<i32> = Vec::new();
    // Each round stops what the previous one found; a process tree is
    // finite, the bound only guards against a scan that keeps changing.
    for _ in 0..16 {
        let new: Vec<i32> = same_group_descendants(pid)
            .into_iter()
            .filter(|p| !stopped.contains(p))
            .collect();
        if new.is_empty() {
            break;
        }
        for p in &new {
            signal(*p, libc::SIGSTOP);
        }
        stopped.extend(new);
    }
    for p in std::iter::once(pid).chain(stopped.iter().copied()) {
        signal(p, libc::SIGHUP);
        signal(p, libc::SIGCONT);
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn hang_up(pid: i32) {
    signal(pid, libc::SIGHUP);
}

fn signal(pid: i32, sig: libc::c_int) {
    // SAFETY: plain syscall. `pid` is our unreaped child or was found as
    // one of its descendants a moment ago; like `pkill -P`, this accepts
    // the tiny window in which a descendant exits and its pid is reused.
    unsafe {
        libc::kill(pid, sig);
    }
}

/// The descendants of `pid` that are still in its process group, children
/// before grandchildren.
#[cfg(target_os = "linux")]
fn same_group_descendants(pid: i32) -> Vec<i32> {
    let table = linux::processes();
    let Some(group) = table.iter().find(|p| p.pid == pid).map(|p| p.group) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut next = vec![pid];
    while let Some(parent) = next.pop() {
        for p in table.iter().filter(|p| p.parent == parent) {
            if p.group == group && !found.contains(&p.pid) {
                found.push(p.pid);
                next.push(p.pid);
            }
        }
    }
    found
}

#[cfg(target_os = "linux")]
mod linux {
    /// One live process as `/proc/<pid>/stat` describes it.
    pub(super) struct Proc {
        pub pid: i32,
        pub parent: i32,
        pub group: i32,
    }

    /// Every process that runs right now (zombies left out).
    pub(super) fn processes() -> Vec<Proc> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                parse_stat(pid, &stat)
            })
            .collect()
    }

    /// Parses `pid (comm) state ppid pgrp ...`; the command name may hold
    /// spaces and parentheses, so fields are read after its last `)`.
    pub(super) fn parse_stat(pid: i32, stat: &str) -> Option<Proc> {
        let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
        if fields.next()? == "Z" {
            return None;
        }
        let parent = fields.next()?.parse().ok()?;
        let group = fields.next()?.parse().ok()?;
        Some(Proc { pid, parent, group })
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn stat_lines_with_odd_command_names_parse() {
        let p = linux::parse_stat(7, "7 (a) b (c)) S 3 5 5 0").expect("parses");
        assert_eq!((p.pid, p.parent, p.group), (7, 3, 5));
        assert!(linux::parse_stat(8, "8 (x) Z 3 5 5 0").is_none());
        assert!(linux::parse_stat(9, "garbage").is_none());
    }

    #[test]
    fn finds_a_shells_child_in_its_group_but_not_other_groups() {
        // `setsid` moves the second sleep into a group (and session) of
        // its own.
        let mut sh = Command::new("/bin/sh")
            .args(["-c", "sleep 30 & setsid sleep 30 & wait"])
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn sh");
        let pid = sh.id() as i32;
        // Both children have run `sleep` once they show its command line
        // (the second only after `setsid` made its new group).
        let is_sleep = |pid: i32| {
            std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| c.starts_with("sleep\0"))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let children = loop {
            let all: Vec<_> = linux::processes()
                .into_iter()
                .filter(|p| p.parent == pid && is_sleep(p.pid))
                .collect();
            if all.len() == 2 || Instant::now() > deadline {
                break all;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let found = same_group_descendants(pid);
        children.iter().for_each(|c| signal(c.pid, libc::SIGKILL));
        let _ = sh.wait();
        assert_eq!(children.len(), 2, "the shell did not start both");
        for child in &children {
            let same_group = child.group != child.pid;
            assert_eq!(found.contains(&child.pid), same_group, "{}", child.pid);
        }
    }

    #[test]
    fn hanging_up_a_fresh_shell_leaves_nothing_behind() {
        for round in 0..20u64 {
            // A duration no other process uses marks this round's sleeps.
            let marker = format!("30.{}{round:02}", std::process::id());
            let mut sh = Command::new("/bin/sh")
                .args(["-c", &format!("sleep {marker}; sleep {marker}")])
                .stdin(Stdio::null())
                .spawn()
                .expect("spawn sh");
            // Growing delays: the hangup lands before, during and after
            // the shell forks its sleep.
            std::thread::sleep(Duration::from_micros(round * 500));
            hang_up(sh.id() as i32);
            let _ = sh.wait();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let leaked = sleeps_with(&marker);
                if leaked.is_empty() {
                    break;
                }
                if Instant::now() > deadline {
                    leaked.iter().for_each(|p| signal(*p, libc::SIGKILL));
                    panic!("the hangup orphaned {leaked:?}");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Live `sleep <marker>` processes.
    fn sleeps_with(marker: &str) -> Vec<i32> {
        let cmdline = format!("sleep\0{marker}\0");
        linux::processes()
            .into_iter()
            .map(|p| p.pid)
            .filter(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == cmdline)
            })
            .collect()
    }
}
