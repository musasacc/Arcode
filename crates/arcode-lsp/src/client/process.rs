//! Spawning a server process and supervising it.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use lsp_types::MessageType;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;

use super::group::{GroupGuard, own_group};
use super::shared::Shared;
use crate::error::{LspError, Result};

/// How to launch a language server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerSpec {
    /// Name used in events and logs (e.g. `"rust-analyzer"`).
    pub name: String,
    /// Executable name or path; looked up on `PATH` (on Windows also with
    /// the `PATHEXT` extensions, so `.cmd`/`.bat` shims are found).
    pub command: String,
    /// Command-line arguments. `${workspaceFolder}` is replaced by the
    /// server's root folder and `${workspaceFolderHash}` by a short, stable
    /// hash of it (for per-workspace state folders).
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Sent as `initializationOptions`; also answers `workspace/configuration`.
    pub initialization_options: Option<Value>,
    /// Features the client uses from this server (all by default).
    pub features: crate::Features,
    /// Set (by a launch hook) when `command` cannot run here, e.g. a rustup
    /// proxy whose component is not installed: starting fails with this
    /// reason instead of running it.
    pub missing: Option<String>,
}

impl ServerSpec {
    /// A spec with just a name and a command.
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        ServerSpec {
            name: name.into(),
            command: command.into(),
            ..Default::default()
        }
    }
}

/// Pipes of a freshly spawned server.
pub(crate) struct Spawned {
    pub(crate) child: Child,
    /// Kills what the server started; see [`super::group`].
    pub(crate) group: GroupGuard,
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: Option<ChildStderr>,
}

/// Spawns the server with piped stdio in `root` (if it exists).
pub(crate) fn spawn(spec: &ServerSpec, root: &Path) -> Result<Spawned> {
    if let Some(reason) = &spec.missing {
        return Err(LspError::NotFound(reason.clone()));
    }
    // Windows: find `.cmd`/`.bat` shims via PATHEXT (std only tries `.exe`).
    #[cfg(windows)]
    let program = super::resolve::resolve_native(&spec.command, &spec.env)
        .map(std::path::PathBuf::into_os_string)
        .unwrap_or_else(|| spec.command.clone().into());
    #[cfg(not(windows))]
    let program = std::ffi::OsString::from(&spec.command);
    let mut cmd = Command::new(&program);
    cmd.args(spec.args.iter().map(|arg| expand_root_vars(arg, root)))
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if root.is_dir() {
        cmd.current_dir(root);
    }
    own_group(&mut cmd);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: do not flash a console window for the server.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            LspError::NotFound(format!(
                "{} not found on PATH (install it or set the full path): {e}",
                spec.command
            ))
        } else {
            LspError::Io(format!("failed to start {}: {e}", spec.command))
        }
    })?;
    let group = GroupGuard::new(child.id());
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    match (stdin, stdout) {
        (Some(stdin), Some(stdout)) => Ok(Spawned {
            child,
            group,
            stdin,
            stdout,
            stderr,
        }),
        _ => Err(LspError::Io("server stdio was not captured".into())),
    }
}

/// Replaces `${workspaceFolder}` and `${workspaceFolderHash}` in `arg`.
pub(crate) fn expand_root_vars(arg: &str, root: &Path) -> String {
    if !arg.contains("${workspaceFolder") {
        return arg.to_string();
    }
    let root_text = root.to_string_lossy();
    arg.replace("${workspaceFolderHash}", &root_hash(&root_text))
        .replace("${workspaceFolder}", &root_text)
}

/// 16 hex digits of the FNV-1a hash of `root`: the same on every run and
/// every platform, unlike `std`'s hasher.
fn root_hash(root: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Longest stderr chunk forwarded as one `Log` event; longer lines are split.
const MAX_STDERR_LINE: u64 = 64 * 1024;

/// Forwards stderr lines as `Log` events until EOF.
///
/// Bytes are decoded lossily (servers may write in a legacy code page), and
/// the pipe is drained to the end so the server never hits `EPIPE`/`SIGPIPE`.
pub(crate) async fn forward_stderr(shared: Arc<Shared>, stderr: ChildStderr) {
    forward_lines(stderr, |line| {
        shared.note_stderr(&line);
        shared.log(MessageType::LOG, line);
    })
    .await;
    shared.stderr_closed();
    log::trace!("[{}] stderr closed", shared.name);
}

/// Calls `emit` for every line of `reader` (without its line ending) until
/// EOF or an unrecoverable read error.
pub(crate) async fn forward_lines<R>(reader: R, mut emit: impl FnMut(String))
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut errors = 0u32;
    loop {
        buf.clear();
        match (&mut reader)
            .take(MAX_STDERR_LINE)
            .read_until(b'\n', &mut buf)
            .await
        {
            Ok(0) => return,
            Ok(_) => {
                errors = 0;
                while matches!(buf.last(), Some(b'\n' | b'\r')) {
                    buf.pop();
                }
                emit(String::from_utf8_lossy(&buf).into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                // Transient errors are retried a few times; a pipe that keeps
                // failing is dead anyway.
                errors += 1;
                log::debug!("stderr read failed: {e}");
                if errors >= 8 {
                    return;
                }
            }
        }
    }
}

/// Waits for the child to exit, or kills it when `kill` fires or is dropped.
/// Whatever the server started dies with `group` once the server is gone,
/// or when this future is dropped.
pub(crate) async fn supervise(
    shared: Arc<Shared>,
    mut child: Child,
    group: GroupGuard,
    kill: oneshot::Receiver<()>,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = kill => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    // Before reporting the stop: whoever waits for it may exit right after.
    drop(group);
    let reason = match status {
        Ok(s) => format!("server exited ({s})"),
        Err(e) => format!("waiting for server failed: {e}"),
    };
    shared.terminate_exited(reason).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn lines_of(bytes: &'static [u8]) -> Vec<String> {
        let mut out = Vec::new();
        forward_lines(bytes, |l| out.push(l)).await;
        out
    }

    #[tokio::test]
    async fn stderr_lines_survive_invalid_utf8_and_crlf() {
        let got = lines_of(b"ok\r\nbad \xff byte\nafter\nno newline").await;
        assert_eq!(got, ["ok", "bad \u{fffd} byte", "after", "no newline"]);
    }

    #[tokio::test]
    async fn very_long_lines_are_split() {
        let long: &'static [u8] = Box::leak(vec![b'x'; 70 * 1024].into_boxed_slice());
        let got = lines_of(long).await;
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len() as u64, MAX_STDERR_LINE);
        assert_eq!(got[0].len() + got[1].len(), 70 * 1024);
    }

    #[tokio::test]
    async fn empty_stream_emits_nothing() {
        assert!(lines_of(b"").await.is_empty());
    }

    #[test]
    fn workspace_folder_variables() {
        let root = Path::new("/home/me/project");
        assert_eq!(expand_root_vars("--stdio", root), "--stdio");
        assert_eq!(
            expand_root_vars("--root=${workspaceFolder}", root),
            "--root=/home/me/project"
        );
        let data = expand_root_vars("/state/jdtls/${workspaceFolderHash}", root);
        let hash = data.strip_prefix("/state/jdtls/").unwrap();
        assert_eq!(hash.len(), 16);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        // Stable for a root, different for another.
        assert_eq!(
            data,
            expand_root_vars("/state/jdtls/${workspaceFolderHash}", root)
        );
        assert_ne!(
            data,
            expand_root_vars(
                "/state/jdtls/${workspaceFolderHash}",
                Path::new("/home/me/other")
            )
        );
        assert_eq!(root_hash(""), "cbf29ce484222325");
    }
}
