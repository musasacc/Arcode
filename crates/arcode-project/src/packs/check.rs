//! Checking which pack components are installed.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{CheckCommand, Component, ComponentKind, LanguagePack, Os};
use crate::command::args;

/// How long one check command may run before it counts as unknown.
pub const DEFAULT_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// The result of running a check command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// Exited with status 0; carries the (truncated) standard output.
    Success {
        /// Standard output, lossy UTF-8.
        stdout: String,
    },
    /// Exited with a non-zero status.
    Failed {
        /// Exit code, if the process was not killed by a signal.
        code: Option<i32>,
    },
    /// The program is not on `PATH`.
    NotFound,
    /// Still running at the timeout; it was killed.
    TimedOut,
    /// Could not be started for another reason.
    Error(String),
}

/// Runs check commands. Tests substitute a stub.
pub trait CommandRunner: Sync {
    /// Runs `program` with `args`, without stdin, for at most `timeout`.
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> RunOutcome;
}

/// Runs commands on this machine: stdin is null, stderr discarded, the
/// working directory is the temp dir (so no project toolchain file triggers
/// a download), and auto-downloads are disabled via `GOTOOLCHAIN=local` and
/// `RUSTUP_AUTO_INSTALL=0`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> RunOutcome {
        let Some(path) = find_executable(program) else {
            return RunOutcome::NotFound;
        };
        let mut command = Command::new(path);
        command
            .args(args)
            .current_dir(std::env::temp_dir())
            .env("GOTOOLCHAIN", "local")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("NO_UPDATE_NOTIFIER", "1")
            .env("npm_config_update_notifier", "false")
            .env("npm_config_offline", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return RunOutcome::NotFound,
            Err(e) => return RunOutcome::Error(e.to_string()),
        };
        let (tx, rx) = mpsc::channel();
        if let Some(mut stdout) = child.stdout.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = (&mut stdout).take(64 * 1024).read_to_end(&mut buf);
                let _ = std::io::copy(&mut stdout, &mut std::io::sink());
                let _ = tx.send(buf);
            });
        }
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return RunOutcome::TimedOut;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(e) => return RunOutcome::Error(e.to_string()),
            }
        };
        if !status.success() {
            return RunOutcome::Failed {
                code: status.code(),
            };
        }
        // A grandchild may keep the pipe open; do not wait for it forever.
        let stdout = rx
            .recv_timeout(Duration::from_millis(500))
            .unwrap_or_default();
        RunOutcome::Success {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
        }
    }
}

/// Finds `program` on `PATH` (with `PATHEXT` extensions on Windows). A
/// program containing a path separator is returned as-is if it exists.
pub fn find_executable(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let exts: Vec<OsString> = if cfg!(windows) {
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        std::iter::once(OsString::new())
            .chain(
                pathext
                    .split(';')
                    .filter(|e| !e.is_empty())
                    .map(OsString::from),
            )
            .collect()
    } else {
        vec![OsString::new()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter().find_map(|ext| {
            let mut name = OsString::from(program);
            name.push(ext);
            let candidate = dir.join(name);
            is_executable(&candidate).then_some(candidate)
        })
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Whether a component is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolState {
    /// The check command succeeded.
    Installed,
    /// The program is not on `PATH`, or its check exited non-zero.
    Missing,
    /// No check for this OS, or the check timed out or failed to start.
    Unknown,
}

/// The state of one component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStatus {
    /// Component name.
    pub component: String,
    /// Its role.
    pub kind: ComponentKind,
    /// Installed, missing or unknown.
    pub state: ToolState,
    /// First line of the check's output when installed (usually a version).
    pub version: Option<String>,
}

/// Checks every component of `pack` on this OS with [`DEFAULT_CHECK_TIMEOUT`].
///
/// Blocks (checks run in parallel threads): call it off the UI thread.
pub fn check_installed(pack: &LanguagePack, runner: &dyn CommandRunner) -> Vec<ComponentStatus> {
    check_installed_on(pack, Os::current(), runner, DEFAULT_CHECK_TIMEOUT)
}

/// Like [`check_installed`] for a given OS and timeout.
pub fn check_installed_on(
    pack: &LanguagePack,
    os: Os,
    runner: &dyn CommandRunner,
    timeout: Duration,
) -> Vec<ComponentStatus> {
    thread::scope(|scope| {
        let handles: Vec<_> = pack
            .components
            .iter()
            .map(|c| scope.spawn(move || check_component(c, os, runner, timeout)))
            .collect();
        handles
            .into_iter()
            .zip(&pack.components)
            .map(|(h, c)| {
                h.join()
                    .unwrap_or_else(|_| status(c, ToolState::Unknown, None))
            })
            .collect()
    })
}

/// Checks one component.
pub fn check_component(
    component: &Component,
    os: Os,
    runner: &dyn CommandRunner,
    timeout: Duration,
) -> ComponentStatus {
    let Some(check) = component.check_for(os) else {
        return status(component, ToolState::Unknown, None);
    };
    let (state, version) = run_check(check, runner, timeout);
    status(component, state, version)
}

fn status(component: &Component, state: ToolState, version: Option<String>) -> ComponentStatus {
    ComponentStatus {
        component: component.name.clone(),
        kind: component.kind,
        state,
        version,
    }
}

fn run_check(
    check: &CheckCommand,
    runner: &dyn CommandRunner,
    timeout: Duration,
) -> (ToolState, Option<String>) {
    match runner.run(&check.program, &check.args, timeout) {
        RunOutcome::Success { stdout } => {
            let first = stdout.lines().map(str::trim).find(|l| !l.is_empty());
            (ToolState::Installed, first.map(str::to_string))
        }
        RunOutcome::Failed { .. } | RunOutcome::NotFound => (ToolState::Missing, None),
        RunOutcome::TimedOut | RunOutcome::Error(_) => (ToolState::Unknown, None),
    }
}

/// The check for a prerequisite named in [`Component::requires`]
/// (`npm`, `node`, `python`, `go`, `rustup`, `cargo`).
pub fn requirement_check(name: &str, os: Os) -> Option<CheckCommand> {
    let (program, list): (&str, &[&str]) = match name {
        "npm" | "node" | "rustup" | "cargo" => (name, &["--version"]),
        "python" => (os.python(), &["--version"]),
        "go" => ("go", &["version"]),
        _ => return None,
    };
    Some(CheckCommand {
        os: vec![os],
        program: program.to_string(),
        args: args(list),
    })
}

/// Checks every distinct prerequisite of the pack's components on `os`.
pub fn check_requirements(
    pack: &LanguagePack,
    os: Os,
    runner: &dyn CommandRunner,
) -> Vec<(String, ToolState)> {
    let mut names: Vec<&String> = pack.components.iter().flat_map(|c| &c.requires).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let state = requirement_check(name, os).map_or(ToolState::Unknown, |c| {
                run_check(&c, runner, DEFAULT_CHECK_TIMEOUT).0
            });
            (name.clone(), state)
        })
        .collect()
}
