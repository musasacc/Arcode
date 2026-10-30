//! The locked-down WASI 0.2 context of a plugin instance (§4 "WASI context").
//!
//! - no environment variables, arguments or stdin;
//! - stdout and stderr go to the plugin log (line-buffered, capped);
//! - clocks and random are allowed;
//! - TCP, UDP and name lookup are off, and every socket address is denied;
//! - `/project` is preopened only when granted, read-only unless write was
//!   granted too. cap-std refuses `..`, absolute-path and symlink escapes.

use std::path::Path;

use bytes::Bytes;
use tokio::io::AsyncWrite;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamResult};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder};

use crate::consent::FileGrant;
use crate::plugin_log::{LogSource, PluginLog};

/// Where the project appears inside the sandbox.
pub const PROJECT_GUEST_PATH: &str = "/project";

/// How much a guest may write per `check-write` permit.
const WRITE_PERMIT: usize = 64 * 1024;

/// Builds the WASI context for one instance.
pub fn build(
    log: &PluginLog,
    project_root: Option<&Path>,
    files: FileGrant,
) -> wasmtime::Result<WasiCtx> {
    let mut b = WasiCtxBuilder::new();
    b.stdout(LogStream::new(log, LogSource::Stdout))
        .stderr(LogStream::new(log, LogSource::Stderr))
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false)
        .socket_addr_check(|_, _| Box::pin(async { false }))
        .allow_blocking_current_thread(true);
    let perms = match files {
        FileGrant::None => None,
        FileGrant::Read => Some(FsPerms::ReadOnly),
        FileGrant::ReadWrite => Some(FsPerms::ReadWrite),
    };
    if let (Some(root), Some(perms)) = (project_root, perms) {
        b.preopened_dir(root, PROJECT_GUEST_PATH, perms)?;
    }
    Ok(b.build())
}

/// Guest stdout/stderr feeding a [`PluginLog`]. Writes never block and
/// never fail: the log drops its oldest lines instead.
#[derive(Debug, Clone)]
struct LogStream {
    log: PluginLog,
    source: LogSource,
}

impl LogStream {
    fn new(log: &PluginLog, source: LogSource) -> Self {
        Self {
            log: log.clone(),
            source,
        }
    }
}

impl IsTerminal for LogStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LogStream {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

#[wasmtime_wasi::async_trait]
impl Pollable for LogStream {
    async fn ready(&mut self) {}
}

#[wasmtime_wasi::async_trait]
impl OutputStream for LogStream {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.log.write_stream(self.source, &bytes);
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(WRITE_PERMIT)
    }
}

/// Required by [`StdoutStream`]; only used by async WASI (p3), never here.
impl AsyncWrite for LogStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.log.write_stream(self.source, buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
