//! [`SystemRunner`]: runs check commands on this machine, offline.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::{CommandRunner, RunOutcome};

/// An address nothing listens on (TCP discard port on loopback): proxy-aware
/// HTTP clients fail at once instead of downloading.
const DEAD_PROXY: &str = "http://127.0.0.1:9";

/// How long to wait for output after the process exited.
const OUTPUT_GRACE: Duration = Duration::from_millis(500);

/// Runs commands on this machine without touching the network.
///
/// stdin is null and stderr discarded; the working directory is the temp dir
/// (so no project toolchain file triggers a download). Auto-downloads are
/// disabled (`GOTOOLCHAIN=local`, `GOPROXY=off`, `RUSTUP_AUTO_INSTALL=0`,
/// npm offline, `PIP_NO_INDEX`), and every HTTP(S) proxy variable points at a
/// dead loopback address, so wrappers that would fetch on first run (such as
/// the PyPI `pyright` wrapper) fail fast instead. On Unix the command runs
/// in its own process group; on timeout the whole group (on Windows the
/// whole process tree) is killed, so proxies' children do not linger.
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        offline_env(&mut command);
        isolate(&mut command);
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
                    kill_tree(&child);
                    let _ = child.kill();
                    let _ = child.wait();
                    return RunOutcome::TimedOut;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(e) => return RunOutcome::Error(e.to_string()),
            }
        };
        // A leftover grandchild may keep the pipe open: do not wait for it,
        // and end it so the reader thread finishes.
        let stdout = match rx.recv_timeout(OUTPUT_GRACE) {
            Ok(stdout) => stdout,
            Err(_) => {
                kill_group_after_exit(&child);
                Vec::new()
            }
        };
        if !status.success() {
            return RunOutcome::Failed {
                code: status.code(),
            };
        }
        RunOutcome::Success {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
        }
    }
}

/// Environment that keeps known tools from downloading anything.
fn offline_env(command: &mut Command) {
    command
        .env("GOTOOLCHAIN", "local")
        .env("GOPROXY", "off")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env("NO_UPDATE_NOTIFIER", "1")
        .env("npm_config_update_notifier", "false")
        .env("npm_config_offline", "true")
        .env("npm_config_proxy", DEAD_PROXY)
        .env("npm_config_https_proxy", DEAD_PROXY)
        .env("PIP_NO_INDEX", "1")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
    for var in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        command
            .env(var, DEAD_PROXY)
            .env(var.to_ascii_lowercase(), DEAD_PROXY);
    }
    command.env_remove("NO_PROXY").env_remove("no_proxy");
}

#[cfg(unix)]
fn isolate(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn isolate(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn isolate(_command: &mut Command) {}

/// Kills the child's process group (Unix) or process tree (Windows) while
/// the child is still running.
#[cfg(unix)]
fn kill_tree(child: &Child) {
    kill_group(child);
}

#[cfg(windows)]
fn kill_tree(child: &Child) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let taskkill = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("taskkill.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("taskkill"));
    let _ = Command::new(taskkill)
        .args(["/F", "/T", "/PID", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_tree(_child: &Child) {}

/// Kills what is left of the process group after the child exited. Its id
/// cannot be reused while the group still has members.
#[cfg(unix)]
fn kill_group_after_exit(child: &Child) {
    kill_group(child);
}

/// Windows cannot find the tree of an exited process; the reader thread
/// ends when the last holder of the pipe exits.
#[cfg(not(unix))]
fn kill_group_after_exit(_child: &Child) {}

#[cfg(unix)]
fn kill_group(child: &Child) {
    let Ok(pgid) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    // SAFETY: `kill` has no memory-safety preconditions; a negative pid
    // targets the process group the child leads (created by `process_group(0)`).
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

/// Finds `program` on `PATH` (with `PATHEXT` extensions on Windows). A
/// program containing a path separator is returned as-is if it exists.
///
/// On Windows the bare name is tried only after the `PATHEXT` extensions
/// and only when it already has an extension: npm installs an extensionless
/// shell script next to every `<name>.cmd`, which Windows cannot run.
pub fn find_executable(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let exts = candidate_extensions(direct);
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter().find_map(|ext| {
            let mut name = OsString::from(program);
            name.push(ext);
            let candidate = dir.join(name);
            is_executable(&candidate).then_some(candidate)
        })
    })
}

/// Suffixes to append to a program name, in lookup order.
fn candidate_extensions(program: &Path) -> Vec<OsString> {
    if !cfg!(windows) {
        return vec![OsString::new()];
    }
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let mut exts: Vec<OsString> = pathext
        .split(';')
        .filter(|e| !e.is_empty())
        .map(OsString::from)
        .collect();
    if program.extension().is_some() {
        exts.push(OsString::new());
    }
    exts
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
