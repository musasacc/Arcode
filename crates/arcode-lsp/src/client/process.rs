//! Spawning a server process and supervising it.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use lsp_types::MessageType;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;

use super::shared::Shared;
use crate::error::{LspError, Result};

/// How to launch a language server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerSpec {
    /// Name used in events and logs (e.g. `"rust-analyzer"`).
    pub name: String,
    /// Executable name or path; looked up on `PATH`.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Sent as `initializationOptions`; also answers `workspace/configuration`.
    pub initialization_options: Option<Value>,
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
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: Option<ChildStderr>,
}

/// Spawns the server with piped stdio in `root` (if it exists).
pub(crate) fn spawn(spec: &ServerSpec, root: &Path) -> Result<Spawned> {
    let mut cmd = Command::new(&spec.command);
    cmd.args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if root.is_dir() {
        cmd.current_dir(root);
    }
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
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    match (stdin, stdout) {
        (Some(stdin), Some(stdout)) => Ok(Spawned {
            child,
            stdin,
            stdout,
            stderr,
        }),
        _ => Err(LspError::Io("server stdio was not captured".into())),
    }
}

/// Forwards stderr lines as `Log` events until EOF.
pub(crate) async fn forward_stderr(shared: Arc<Shared>, stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => shared.log(MessageType::LOG, line),
            Ok(None) => return,
            Err(e) => {
                log::debug!("[{}] stderr read failed: {e}", shared.name);
                return;
            }
        }
    }
}

/// Waits for the child to exit, or kills it when `kill` fires or is dropped.
pub(crate) async fn supervise(shared: Arc<Shared>, mut child: Child, kill: oneshot::Receiver<()>) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = kill => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    let reason = match status {
        Ok(s) => format!("server exited ({s})"),
        Err(e) => format!("waiting for server failed: {e}"),
    };
    shared.terminate(reason);
}
