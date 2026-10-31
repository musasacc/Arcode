//! Review: `reaper::tests::terminate_hangs_up_a_running_child` leaks its
//! `sleep 30` whenever `/bin/sh` (dash) has already forked it before the
//! SIGHUP arrives, which happens under load (the whole lib suite in
//! parallel, CI). Only the shell is hung up; its child is orphaned and
//! keeps running, which the new CI leak check reports as a failure.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::reaper::terminate;

fn children_of(pid: u32) -> Vec<i32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect()
}

fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            !s.rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        })
        .unwrap_or(false)
}

#[test]
#[cfg(target_os = "linux")]
fn review_terminate_test_scenario_leaves_no_sleep_behind() {
    // Exactly the existing test's child, but the shell got to run first
    // (as it does when the suite runs in parallel on a busy runner).
    let mut child = Command::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn sh");
    std::thread::sleep(Duration::from_millis(200));
    let grandchildren = children_of(child.id());
    terminate(child.id(), Duration::from_secs(5));
    let _ = child.wait();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut leaked: Vec<i32> = grandchildren.clone();
    while Instant::now() < deadline {
        leaked.retain(|p| alive(*p));
        if leaked.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for p in &leaked {
        // Clean up so this review test itself does not leak.
        unsafe { libc::kill(*p, libc::SIGKILL) };
    }
    assert!(
        leaked.is_empty(),
        "the test scenario orphaned {leaked:?} (sh's `sleep 30`), grandchildren were {grandchildren:?}"
    );
}
