//! Opening a transport to an adapter: spawning its process (stdio) or
//! connecting over TCP, and supervising the process.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

use crate::config::{AdapterCommand, AdapterSpec, AdapterTransport, PORT_PLACEHOLDER};
use crate::connection::Connection;
use crate::error::{DapError, Result};

/// After the process exits, how long the reader may still drain messages.
const EXIT_GRACE: Duration = Duration::from_millis(500);
/// Delay between TCP connection attempts.
const CONNECT_RETRY: Duration = Duration::from_millis(50);
/// Longest log chunk forwarded as one line; longer lines are split.
const MAX_LOG_LINE: u64 = 64 * 1024;

/// Boxed reading half of a transport.
pub(crate) type BoxReader = Box<dyn AsyncBufRead + Unpin + Send>;
/// Boxed writing half of a transport.
pub(crate) type BoxWriter = Box<dyn AsyncWrite + Unpin + Send>;
/// A process output stream forwarded as log lines.
pub(crate) type BoxLog = Box<dyn AsyncRead + Unpin + Send>;

/// An open transport and the process behind it, if any.
pub(crate) struct Opened {
    pub(crate) reader: BoxReader,
    pub(crate) writer: BoxWriter,
    pub(crate) child: Option<Child>,
    pub(crate) logs: Vec<BoxLog>,
}

/// Spawns and/or connects to the adapter described by `spec`.
pub(crate) async fn open(spec: &AdapterSpec) -> Result<Opened> {
    match &spec.transport {
        AdapterTransport::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let cmd = AdapterCommand {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
                cwd: cwd.clone(),
            };
            let mut child = spawn(spec, &cmd, true)?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                return Err(DapError::Io("adapter stdio was not captured".into()));
            };
            let logs = child.stderr.take().map(|e| Box::new(e) as BoxLog);
            Ok(Opened {
                reader: Box::new(BufReader::new(stdout)),
                writer: Box::new(stdin),
                child: Some(child),
                logs: logs.into_iter().collect(),
            })
        }
        AdapterTransport::Tcp {
            command,
            host,
            port,
            connect_timeout,
        } => {
            let port = match *port {
                0 => free_port(host)?,
                p => p,
            };
            let mut child = match command {
                Some(cmd) => {
                    let mut cmd = cmd.clone();
                    for arg in &mut cmd.args {
                        *arg = arg.replace(PORT_PLACEHOLDER, &port.to_string());
                    }
                    Some(spawn(spec, &cmd, false)?)
                }
                None => None,
            };
            let stream = connect(spec, host, port, *connect_timeout, child.as_mut()).await?;
            let (read, write) = stream.into_split();
            let mut logs = Vec::new();
            if let Some(child) = child.as_mut() {
                logs.extend(child.stdout.take().map(|o| Box::new(o) as BoxLog));
                logs.extend(child.stderr.take().map(|e| Box::new(e) as BoxLog));
            }
            Ok(Opened {
                reader: Box::new(BufReader::new(read)),
                writer: Box::new(write),
                child,
                logs,
            })
        }
    }
}

/// Spawns `cmd`; with `stdio` the protocol runs over stdin/stdout,
/// otherwise stdin is null and stdout is a log stream.
fn spawn(spec: &AdapterSpec, cmd: &AdapterCommand, stdio: bool) -> Result<Child> {
    let program = crate::resolve::find_executable(&cmd.command, &cmd.env)
        .ok_or_else(|| spec.not_found_error())?;
    let mut command = Command::new(&program);
    command
        .args(&cmd.args)
        .envs(cmd.env.iter().map(|(k, v)| (k, v)))
        .stdin(if stdio { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = cmd.cwd.as_ref().filter(|d| d.is_dir()) {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: do not flash a console window for the adapter.
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map_err(|e| DapError::Io(format!("failed to start {}: {e}", program.display())))
}

/// Asks the OS for a free port on `host`.
fn free_port(host: &str) -> Result<u16> {
    let listener = std::net::TcpListener::bind((host, 0))
        .map_err(|e| DapError::Io(format!("no free port on {host}: {e}")))?;
    Ok(listener.local_addr()?.port())
}

/// Connects, retrying until `timeout` while the adapter starts listening.
async fn connect(
    spec: &AdapterSpec,
    host: &str,
    port: u16,
    timeout: Duration,
    mut child: Option<&mut Child>,
) -> Result<TcpStream> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let error = match TcpStream::connect((host, port)).await {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(e) => e,
        };
        if let Some(child) = child.as_deref_mut()
            && let Ok(Some(status)) = child.try_wait()
        {
            let tail = match child.stderr.take() {
                Some(stderr) => read_tail(stderr).await,
                None => String::new(),
            };
            return Err(DapError::Io(format!(
                "{} exited ({status}) before accepting connections{tail}",
                spec.name
            )));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(DapError::Io(format!(
                "could not connect to {} at {host}:{port}: {error}",
                spec.name
            )));
        }
        tokio::time::sleep(CONNECT_RETRY).await;
    }
}

/// Reads what an exited process left on a pipe, formatted as `": last lines"`.
async fn read_tail(stream: impl AsyncRead + Unpin) -> String {
    let mut buf = Vec::new();
    let mut limited = stream.take(64 * 1024);
    let _ = tokio::time::timeout(EXIT_GRACE, limited.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(5)..].join("\n");
    if tail.is_empty() {
        tail
    } else {
        format!(": {tail}")
    }
}

/// Waits for the process to exit, or kills it when `kill` fires or is
/// dropped; then terminates the connection (after letting the reader
/// drain the final messages).
pub(crate) async fn supervise(
    conn: Arc<Connection>,
    mut child: Child,
    kill: oneshot::Receiver<()>,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = kill => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    let _ = tokio::time::timeout(EXIT_GRACE, conn.closed()).await;
    conn.terminate(match status {
        Ok(s) => format!("adapter exited ({s})"),
        Err(e) => format!("waiting for the adapter failed: {e}"),
    });
}

/// Calls `emit` for every line of `reader` (without its line ending) until
/// EOF. Bytes are decoded lossily and the pipe is drained to the end so
/// the adapter never blocks on a full pipe.
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
            .take(MAX_LOG_LINE)
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
                errors += 1;
                log::debug!("adapter log read failed: {e}");
                if errors >= 8 {
                    return;
                }
            }
        }
    }
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
    async fn log_lines_survive_invalid_utf8_and_crlf() {
        let got = lines_of(b"ok\r\nbad \xff byte\n\nlast").await;
        assert_eq!(got, ["ok", "bad \u{fffd} byte", "", "last"]);
    }

    #[tokio::test]
    async fn tail_keeps_the_last_nonempty_lines() {
        assert_eq!(read_tail(&b""[..]).await, "");
        let text = b"1\n2\n\n3\n4\n5\n6\n";
        assert_eq!(read_tail(&text[..]).await, ": 2\n3\n4\n5\n6");
    }

    #[test]
    fn free_port_is_nonzero() {
        assert_ne!(free_port("127.0.0.1").unwrap(), 0);
    }

    #[tokio::test]
    async fn missing_stdio_adapter_reports_install_hint() {
        let spec = AdapterSpec {
            name: "codelldb".into(),
            adapter_id: "lldb".into(),
            transport: AdapterTransport::Stdio {
                command: "arcode-surely-missing-adapter".into(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            install_hint: Some("install the Rust language pack".into()),
            configure_before_launch: false,
        };
        let err = open(&spec).await.err().unwrap();
        assert_eq!(
            err.to_string(),
            "arcode-surely-missing-adapter not found — install the Rust language pack"
        );
    }

    #[tokio::test]
    async fn tcp_connect_times_out_without_listener() {
        let port = free_port("127.0.0.1").unwrap();
        let spec = AdapterSpec {
            name: "remote".into(),
            adapter_id: "go".into(),
            transport: AdapterTransport::Tcp {
                command: None,
                host: "127.0.0.1".into(),
                port,
                connect_timeout: Duration::from_millis(150),
            },
            install_hint: None,
            configure_before_launch: false,
        };
        let err = open(&spec).await.err().unwrap();
        assert!(
            err.to_string().contains("could not connect to remote"),
            "{err}"
        );
    }
}
