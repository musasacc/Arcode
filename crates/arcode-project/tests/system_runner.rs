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
