//! rustup's proxies. rustup puts an executable for every component it
//! knows into `~/.cargo/bin` (a hard link to, or a symlink of, rustup
//! itself), installed or not: `rust-analyzer` is on `PATH` even when the
//! `rust-analyzer` component is missing, and then only prints `error:
//! Unknown binary 'rust-analyzer' in official toolchain '…'` and exits.
//!
//! [`check_proxy`] tells such a placeholder from a real server by asking
//! `rustup which` (no download: `RUSTUP_AUTO_INSTALL=0`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The language servers rustup provides as components (and therefore as
/// proxies on `PATH`).
pub const PROXIED_SERVERS: [&str; 1] = ["rust-analyzer"];

/// How long `rustup which` may take.
pub const WHICH_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether rustup provides server `command` as a component.
pub fn is_proxied(command: &str) -> bool {
    PROXIED_SERVERS.contains(&command)
}

/// The rustup executable behind `path` when `path` is one of its proxies:
/// a symlink that resolves to `rustup`, or the same file as the `rustup`
/// beside it (a hard link). `None` for any other file.
pub fn proxy_target(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path)
        && real.file_stem().is_some_and(|stem| stem == "rustup")
    {
        return Some(real);
    }
    let rustup = path
        .parent()?
        .join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
    same_file(path, &rustup).then_some(rustup)
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.is_file() && a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Without a stable file id: hard links share size and modification time
/// (a real server is far bigger than rustup).
#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => {
            a.is_file()
                && b.is_file()
                && a.len() == b.len()
                && a.modified().ok().is_some()
                && a.modified().ok() == b.modified().ok()
        }
        _ => false,
    }
}

/// What a server command found on `PATH` really is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyCheck {
    /// Not a rustup proxy: run it.
    NotProxy,
    /// A proxy whose component is installed (rustup runs this file).
    Installed(PathBuf),
    /// A proxy that `rustup which` failed for; what rustup said. Usually
    /// the component is missing, but the toolchain itself can be (one a
    /// project's toolchain file names): the caller tells them apart.
    Missing(String),
    /// A proxy, but rustup did not answer (in time): run it and see.
    Unknown,
}

/// Checks server `command`, found at `found` on `PATH`: for a rustup
/// proxy, asks `rustup which <command>` in `cwd` (the project, whose
/// toolchain file picks the toolchain). Blocks for up to `timeout`.
pub fn check_proxy(
    command: &str,
    found: &Path,
    cwd: Option<&Path>,
    timeout: Duration,
) -> ProxyCheck {
    if !is_proxied(command) {
        return ProxyCheck::NotProxy;
    }
    let Some(rustup) = proxy_target(found) else {
        return ProxyCheck::NotProxy;
    };
    match rustup_which(&rustup, command, cwd, timeout) {
        Some((true, out)) => ProxyCheck::Installed(PathBuf::from(first_line(&out))),
        Some((false, err)) => ProxyCheck::Missing(first_line(&err).to_string()),
        None => ProxyCheck::Unknown,
    }
}

/// Runs `rustup which <command>`: whether it succeeded, and its stdout
/// (on success) or stderr. `None` when it could not run or timed out.
fn rustup_which(
    rustup: &Path,
    command: &str,
    cwd: Option<&Path>,
    timeout: Duration,
) -> Option<(bool, String)> {
    let dir = cwd
        .filter(|d| d.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let mut child = Command::new(rustup)
        .args(["which", command])
        .current_dir(dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    // `rustup which` writes one line: the pipes never fill up.
    let mut text = String::new();
    let pipe: Option<Box<dyn Read>> = if status.success() {
        child.stdout.take().map(|p| Box::new(p) as Box<dyn Read>)
    } else {
        child.stderr.take().map(|p| Box::new(p) as Box<dyn Read>)
    };
    if let Some(pipe) = pipe {
        let _ = pipe.take(16 * 1024).read_to_string(&mut text);
    }
    Some((status.success(), text))
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}
