//! Reviewer tests for Windows-only defects (compiled and run on Windows).
#![cfg(windows)]

use std::time::Duration;

use arcode_project::packs::find_executable;
use arcode_project::{CommandRunner, RunCommand, RunKind, RunOutcome, RunSource, SystemRunner};

/// npm (and every `npm i -g` package) installs an extensionless POSIX shell
/// shim next to `<name>.cmd`. `find_executable` tries the bare name first
/// and returns the shim, which Windows cannot execute, so the check reports
/// NotFound/Error instead of Installed.
#[test]
fn review_npm_style_shim_resolves_to_cmd() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("fakenpm"), "#!/bin/sh\necho 1.0\n").expect("write");
    std::fs::write(dir.path().join("fakenpm.cmd"), "@echo off\r\necho 1.0\r\n").expect("write");
    // SAFETY: this test binary has a single test, nothing reads PATH concurrently.
    unsafe { std::env::set_var("PATH", dir.path()) };
    let found = find_executable("fakenpm").expect("found");
    assert_eq!(
        found.extension().and_then(|e| e.to_str()),
        Some("cmd"),
        "resolved {found:?}"
    );
    let outcome = SystemRunner.run("fakenpm", &[], Duration::from_secs(5));
    assert!(matches!(outcome, RunOutcome::Success { .. }), "{outcome:?}");
}

/// In PowerShell a line starting with a quoted string is an expression, not
/// a command: `'C:\Users\John Doe\...\python.exe' -m pytest` is a parse
/// error (needs `& '...'`).
#[test]
fn review_quoted_program_runs_in_powershell() {
    let cmd = RunCommand::new(
        r"C:\Users\John Doe\proj\.venv\Scripts\python.exe",
        vec!["-m".into(), "pytest".into()],
        r"C:\Users\John Doe\proj",
        RunKind::Test,
        RunSource::Python,
    );
    assert!(
        !cmd.command_line().starts_with('\''),
        "{}",
        cmd.command_line()
    );
}
