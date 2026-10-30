//! The real command runner (spawns small shell commands).

use std::time::Duration;

use arcode_project::packs::find_executable;
use arcode_project::{CommandRunner, RunOutcome, SystemRunner};

fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn missing_program_is_not_found() {
    let outcome = SystemRunner.run(
        "arcode-definitely-not-installed-xyz",
        &[],
        Duration::from_secs(1),
    );
    assert_eq!(outcome, RunOutcome::NotFound);
    assert_eq!(find_executable("arcode-definitely-not-installed-xyz"), None);
}

#[cfg(unix)]
#[test]
fn unix_success_failure_and_timeout() {
    assert!(find_executable("sh").is_some());
    let timeout = Duration::from_secs(5);
    assert_eq!(
        SystemRunner.run("sh", &strs(&["-c", "echo tool 1.2.3"]), timeout),
        RunOutcome::Success {
            stdout: "tool 1.2.3\n".to_string()
        }
    );
    assert_eq!(
        SystemRunner.run("sh", &strs(&["-c", "exit 3"]), timeout),
        RunOutcome::Failed { code: Some(3) }
    );
    // stdin is null, so a reader sees EOF immediately instead of blocking.
    assert_eq!(
        SystemRunner.run("sh", &strs(&["-c", "cat; echo done"]), timeout),
        RunOutcome::Success {
            stdout: "done\n".to_string()
        }
    );
    assert_eq!(
        SystemRunner.run("sh", &strs(&["-c", "sleep 5"]), Duration::from_millis(200)),
        RunOutcome::TimedOut
    );
}

#[cfg(windows)]
#[test]
fn windows_success_and_failure() {
    let timeout = Duration::from_secs(5);
    match SystemRunner.run("cmd", &strs(&["/C", "echo tool"]), timeout) {
        RunOutcome::Success { stdout } => assert!(stdout.contains("tool")),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        SystemRunner.run("cmd", &strs(&["/C", "exit 3"]), timeout),
        RunOutcome::Failed { code: Some(3) }
    );
}

/// Check commands see a dead proxy and no `NO_PROXY`, so wrappers that
/// would download on first run fail fast instead.
#[cfg(unix)]
#[test]
fn unix_checks_run_with_a_dead_proxy() {
    let outcome = SystemRunner.run(
        "sh",
        &strs(&[
            "-c",
            "echo \"$HTTPS_PROXY $https_proxy ${NO_PROXY-unset} $GOPROXY\"",
        ]),
        Duration::from_secs(5),
    );
    assert_eq!(
        outcome,
        RunOutcome::Success {
            stdout: "http://127.0.0.1:9 http://127.0.0.1:9 unset off\n".to_string()
        }
    );
}

/// On timeout the whole process group dies, including grandchildren that
/// hold the output pipe, and the call returns promptly.
#[cfg(unix)]
#[test]
fn unix_timeout_kills_grandchildren() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("survived");
    let script = format!("(sleep 1; touch '{}') & sleep 30", marker.to_string_lossy());
    let start = std::time::Instant::now();
    let outcome = SystemRunner.run("sh", &strs(&["-c", &script]), Duration::from_millis(200));
    assert_eq!(outcome, RunOutcome::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "grandchild survived the timeout");
}

/// A grandchild that outlives a successful check is ended too.
#[cfg(unix)]
#[test]
fn unix_lingering_grandchild_after_success_is_killed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("survived");
    let script = format!(
        "echo v1; (sleep 1; touch '{}') & exit 0",
        marker.to_string_lossy()
    );
    let start = std::time::Instant::now();
    let outcome = SystemRunner.run("sh", &strs(&["-c", &script]), Duration::from_secs(5));
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "waited for grandchild"
    );
    assert_eq!(
        outcome,
        RunOutcome::Success {
            stdout: String::new()
        }
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "grandchild survived");
}
