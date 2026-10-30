//! Known debug adapters and launch-configuration templates.
//!
//! Pure data: nothing is installed or downloaded. [`find_adapter`] checks
//! `PATH` (Windows `PATHEXT`-aware) and explains what to install.
//!
//! | language            | adapters (preference order)          |
//! |---------------------|--------------------------------------|
//! | `rust`, `c`, `cpp`  | codelldb (TCP), lldb-dap, gdb ≥ 14   |
//! | `python`            | debugpy (`python -m debugpy.adapter`)|
//! | `go`                | delve (`dlv dap`, TCP)               |
//! | JS/TS               | js-debug: placeholder, not supported |

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::config::{
    AdapterCommand, AdapterSpec, AdapterTransport, DEFAULT_CONNECT_TIMEOUT, DebugConfig,
    DebugRequest,
};
use crate::error::{DapError, Result};
use crate::resolve::find_executable;

/// A debug adapter Arcode knows how to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownAdapter {
    /// CodeLLDB (`codelldb --port N`), Rust/C/C++.
    CodeLldb,
    /// LLVM's `lldb-dap` (formerly `lldb-vscode`) over stdio.
    LldbDap,
    /// GDB's built-in DAP interpreter (`gdb --interpreter=dap`, GDB 14+).
    Gdb,
    /// Python's debugpy (`python -m debugpy.adapter`).
    Debugpy,
    /// Go's Delve (`dlv dap --listen=127.0.0.1:N`). Delve has no stdio mode.
    Delve,
    /// VS Code's js-debug. Placeholder: it needs `startDebugging` child
    /// sessions, which Arcode does not support yet.
    JsDebug,
}

/// What to launch, independent of the adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Program to debug (binary, script, or Go package directory).
    pub program: PathBuf,
    /// Program arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment variables.
    pub env: BTreeMap<String, String>,
    /// Stop at the program's entry point.
    pub stop_on_entry: bool,
}

impl KnownAdapter {
    /// Every known adapter.
    pub const ALL: [KnownAdapter; 6] = [
        KnownAdapter::CodeLldb,
        KnownAdapter::LldbDap,
        KnownAdapter::Gdb,
        KnownAdapter::Debugpy,
        KnownAdapter::Delve,
        KnownAdapter::JsDebug,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            KnownAdapter::CodeLldb => "codelldb",
            KnownAdapter::LldbDap => "lldb-dap",
            KnownAdapter::Gdb => "gdb",
            KnownAdapter::Debugpy => "debugpy",
            KnownAdapter::Delve => "delve",
            KnownAdapter::JsDebug => "js-debug",
        }
    }

    /// False for placeholders Arcode cannot drive yet.
    pub fn is_supported(self) -> bool {
        self != KnownAdapter::JsDebug
    }

    /// Executable that must be on `PATH`.
    pub fn executable(self) -> &'static str {
        match self {
            KnownAdapter::CodeLldb => "codelldb",
            KnownAdapter::LldbDap => "lldb-dap",
            KnownAdapter::Gdb => "gdb",
            KnownAdapter::Debugpy if cfg!(windows) => "python",
            KnownAdapter::Debugpy => "python3",
            KnownAdapter::Delve => "dlv",
            KnownAdapter::JsDebug => "node",
        }
    }

    fn pack(self) -> &'static str {
        match self {
            KnownAdapter::CodeLldb | KnownAdapter::LldbDap | KnownAdapter::Gdb => "Rust",
            KnownAdapter::Debugpy => "Python",
            KnownAdapter::Delve => "Go",
            KnownAdapter::JsDebug => "JavaScript",
        }
    }

    /// The adapter's spec (transport, `adapterID`, install hint).
    pub fn spec(self) -> AdapterSpec {
        let stdio = |args: &[&str]| AdapterTransport::Stdio {
            command: self.executable().into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
            cwd: None,
        };
        let tcp = |args: &[&str]| AdapterTransport::Tcp {
            command: Some(AdapterCommand {
                command: self.executable().into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                env: Vec::new(),
                cwd: None,
            }),
            host: "127.0.0.1".into(),
            port: 0,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        };
        let (adapter_id, transport) = match self {
            KnownAdapter::CodeLldb => ("lldb", tcp(&["--port", "{port}"])),
            KnownAdapter::LldbDap => ("lldb-dap", stdio(&[])),
            KnownAdapter::Gdb => ("gdb", stdio(&["--interpreter=dap", "--quiet"])),
            KnownAdapter::Debugpy => ("debugpy", stdio(&["-m", "debugpy.adapter"])),
            KnownAdapter::Delve => ("go", tcp(&["dap", "--listen=127.0.0.1:{port}"])),
            KnownAdapter::JsDebug => ("pwa-node", tcp(&["dapDebugServer.js", "{port}", "127.0.0.1"])),
        };
        AdapterSpec {
            name: self.name().into(),
            adapter_id: adapter_id.into(),
            transport,
            install_hint: Some(format!("install the {} language pack", self.pack())),
        }
    }

    /// Launch arguments in the adapter's dialect.
    pub fn launch_arguments(self, options: &LaunchOptions) -> Value {
        let program = options.program.to_string_lossy();
        let cwd = options.cwd.to_string_lossy();
        let (args, env) = (&options.args, &options.env);
        match self {
            KnownAdapter::CodeLldb => json!({
                "type": "lldb", "request": "launch", "program": program, "args": args,
                "cwd": cwd, "env": env, "stopOnEntry": options.stop_on_entry,
                "terminal": "integrated",
            }),
            KnownAdapter::LldbDap => json!({
                "program": program, "args": args, "cwd": cwd, "stopOnEntry": options.stop_on_entry,
                "env": env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(),
            }),
            KnownAdapter::Gdb => json!({
                "program": program, "args": args, "cwd": cwd, "env": env,
                "stopAtBeginningOfMainSubprogram": options.stop_on_entry,
            }),
            KnownAdapter::Debugpy => json!({
                "type": "python", "request": "launch", "program": program, "args": args,
                "cwd": cwd, "env": env, "console": "integratedTerminal",
                "stopOnEntry": options.stop_on_entry, "justMyCode": true,
            }),
            KnownAdapter::Delve => json!({
                "mode": "debug", "program": program, "args": args, "cwd": cwd, "env": env,
                "stopOnEntry": options.stop_on_entry,
            }),
            KnownAdapter::JsDebug => json!({
                "type": "pwa-node", "request": "launch", "program": program, "args": args,
                "cwd": cwd, "env": env, "console": "integratedTerminal",
                "stopOnEntry": options.stop_on_entry,
            }),
        }
    }

    /// A ready-to-start launch config (no breakpoints yet).
    pub fn launch_config(self, options: &LaunchOptions) -> DebugConfig {
        DebugConfig::new(
            self.spec(),
            DebugRequest::Launch,
            self.launch_arguments(options),
        )
    }

    /// True if the executable is on `PATH` (for debugpy this only checks
    /// Python; a missing module shows up when the adapter starts).
    pub fn is_installed(self) -> bool {
        find_executable(self.executable(), &[]).is_some()
    }
}

/// Adapters for an LSP language id, in preference order (may include
/// unsupported placeholders).
pub fn adapters_for_language(language_id: &str) -> &'static [KnownAdapter] {
    use KnownAdapter::*;
    match language_id {
        "rust" | "c" | "cpp" | "objective-c" | "objective-cpp" | "zig" => &[CodeLldb, LldbDap, Gdb],
        "python" => &[Debugpy],
        "go" => &[Delve],
        "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => &[JsDebug],
        _ => &[],
    }
}

/// The preferred supported adapter's spec for a language (pure data).
pub fn adapter_for_language(language_id: &str) -> Option<AdapterSpec> {
    adapters_for_language(language_id)
        .iter()
        .find(|a| a.is_supported())
        .map(|a| a.spec())
}

/// The first supported adapter for the language that is installed.
/// Errors: [`DapError::NotFound`] naming the preferred adapter, e.g.
/// "codelldb not found — install the Rust language pack";
/// [`DapError::Unsupported`] if Arcode cannot debug the language.
pub fn find_adapter(language_id: &str) -> Result<KnownAdapter> {
    find_adapter_with(language_id, KnownAdapter::is_installed)
}

fn find_adapter_with(
    language_id: &str,
    installed: impl Fn(KnownAdapter) -> bool,
) -> Result<KnownAdapter> {
    let supported: Vec<KnownAdapter> = adapters_for_language(language_id)
        .iter()
        .copied()
        .filter(|a| a.is_supported())
        .collect();
    let Some(&preferred) = supported.first() else {
        return Err(DapError::Unsupported(format!(
            "debugging {language_id} is not supported yet"
        )));
    };
    supported
        .into_iter()
        .find(|&a| installed(a))
        .ok_or_else(|| {
            let pack = match language_id {
                "c" | "cpp" | "objective-c" | "objective-cpp" => "C/C++",
                "zig" => "Zig",
                _ => preferred.pack(),
            };
            DapError::NotFound(format!(
                "{} not found — install the {pack} language pack",
                preferred.executable()
            ))
        })
}

#[cfg(test)]
#[path = "adapters_tests.rs"]
mod tests;
