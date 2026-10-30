//! How to reach a debug adapter and what to debug.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::error::{DapError, Result};
use crate::resolve::find_executable;
use crate::types::{FunctionBreakpoint, SourceBreakpoint};

/// Placeholder in TCP adapter arguments replaced by the chosen port.
pub const PORT_PLACEHOLDER: &str = "{port}";

/// Default time to wait for a TCP adapter to accept connections.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A process to spawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdapterCommand {
    /// Executable name (looked up on `PATH`, `PATHEXT`-aware) or path.
    pub command: String,
    /// Arguments; for TCP adapters `{port}` is replaced by the port.
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Working directory (default: inherited).
    pub cwd: Option<PathBuf>,
}

/// How the client talks to the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterTransport {
    /// Spawn the adapter and speak DAP over its stdin/stdout.
    Stdio {
        /// Executable name or path.
        command: String,
        /// Arguments.
        args: Vec<String>,
        /// Extra environment variables.
        env: Vec<(String, String)>,
        /// Working directory.
        cwd: Option<PathBuf>,
    },
    /// Connect over TCP, optionally spawning the adapter first (its stdout
    /// and stderr become [`crate::DebugEvent::AdapterLog`] lines).
    Tcp {
        /// Adapter process to spawn (`None`: it is already listening).
        command: Option<AdapterCommand>,
        /// Host to connect to, normally `127.0.0.1`.
        host: String,
        /// Port; `0` picks a free local port and substitutes it for
        /// `{port}` in the command's arguments.
        port: u16,
        /// How long to retry connecting while the adapter starts.
        connect_timeout: Duration,
    },
}

/// A debug adapter: how to start it and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterSpec {
    /// Display name, e.g. `"codelldb"`.
    pub name: String,
    /// `adapterID` sent in `initialize`, e.g. `"lldb"`.
    pub adapter_id: String,
    /// Transport.
    pub transport: AdapterTransport,
    /// Shown when the executable is missing, e.g. "install the Rust language pack".
    pub install_hint: Option<String>,
}

impl AdapterSpec {
    /// The executable that must exist, if the client spawns one.
    pub fn executable(&self) -> Option<&str> {
        match &self.transport {
            AdapterTransport::Stdio { command, .. } => Some(command),
            AdapterTransport::Tcp { command, .. } => command.as_ref().map(|c| c.command.as_str()),
        }
    }

    fn env(&self) -> &[(String, String)] {
        match &self.transport {
            AdapterTransport::Stdio { env, .. } => env,
            AdapterTransport::Tcp { command, .. } => command.as_ref().map_or(&[], |c| &c.env),
        }
    }

    /// Resolves the executable on `PATH`. `Ok(None)` when nothing needs to
    /// be spawned; [`DapError::NotFound`] with an install hint otherwise,
    /// e.g. "codelldb not found — install the Rust language pack".
    pub fn resolve_executable(&self) -> Result<Option<PathBuf>> {
        let Some(command) = self.executable() else {
            return Ok(None);
        };
        find_executable(command, self.env())
            .map(Some)
            .ok_or_else(|| self.not_found_error())
    }

    /// The error reported when the executable is missing.
    pub fn not_found_error(&self) -> DapError {
        let command = self.executable().unwrap_or(&self.name);
        let hint = self
            .install_hint
            .clone()
            .unwrap_or_else(|| "install it or put it on PATH".into());
        DapError::NotFound(format!("{command} not found — {hint}"))
    }
}

/// Whether the session starts the debuggee or attaches to a running one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugRequest {
    /// `launch`.
    Launch,
    /// `attach`.
    Attach,
}

impl DebugRequest {
    /// The DAP command name.
    pub fn command(self) -> &'static str {
        match self {
            DebugRequest::Launch => "launch",
            DebugRequest::Attach => "attach",
        }
    }
}

/// Everything needed to start a debug session.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugConfig {
    /// The adapter.
    pub adapter: AdapterSpec,
    /// Launch or attach.
    pub request: DebugRequest,
    /// Adapter-specific launch/attach arguments (see [`crate::adapters`]).
    pub arguments: Value,
    /// Initial breakpoints per file (absolute paths).
    pub breakpoints: BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    /// Initial function breakpoints (sent if the adapter supports them).
    pub function_breakpoints: Vec<FunctionBreakpoint>,
    /// Exception filters to enable (sent if the adapter has filters).
    pub exception_filters: Vec<String>,
}

impl DebugConfig {
    /// A config without breakpoints.
    pub fn new(adapter: AdapterSpec, request: DebugRequest, arguments: Value) -> Self {
        DebugConfig {
            adapter,
            request,
            arguments,
            breakpoints: BTreeMap::new(),
            function_breakpoints: Vec::new(),
            exception_filters: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(command: &str, hint: Option<&str>) -> AdapterSpec {
        AdapterSpec {
            name: command.into(),
            adapter_id: "x".into(),
            transport: AdapterTransport::Stdio {
                command: command.into(),
                args: vec![],
                env: vec![("PATH".into(), String::new())],
                cwd: None,
            },
            install_hint: hint.map(Into::into),
        }
    }

    #[test]
    fn missing_executable_names_the_install_hint() {
        let spec = stdio("codelldb", Some("install the Rust language pack"));
        assert_eq!(
            spec.resolve_executable(),
            Err(DapError::NotFound(
                "codelldb not found — install the Rust language pack".into()
            ))
        );
        let spec = stdio("dlv", None);
        assert_eq!(
            spec.resolve_executable().unwrap_err().to_string(),
            "dlv not found — install it or put it on PATH"
        );
    }

    #[test]
    fn tcp_without_command_needs_nothing() {
        let spec = AdapterSpec {
            name: "remote".into(),
            adapter_id: "go".into(),
            transport: AdapterTransport::Tcp {
                command: None,
                host: "127.0.0.1".into(),
                port: 4711,
                connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            },
            install_hint: None,
        };
        assert_eq!(spec.executable(), None);
        assert_eq!(spec.resolve_executable(), Ok(None));
    }

    #[test]
    fn request_commands() {
        assert_eq!(DebugRequest::Launch.command(), "launch");
        assert_eq!(DebugRequest::Attach.command(), "attach");
    }
}
