//! `.arcode/launch.toml`: a project's own launch configurations.
//!
//! ```toml
//! [[configuration]]
//! name = "Debug server"          # shown in the Run and Debug view
//! adapter = "lldb-dap"           # codelldb, lldb-dap, gdb, debugpy, delve
//!                                # (or leave it out and set `language`)
//! language = "rust"              # picks the first installed adapter
//! request = "launch"             # or "attach" (with `pid`)
//! program = "${workspaceFolder}/target/debug/server"
//! args = ["--port", "8080"]
//! cwd = "${workspaceFolder}"
//! env = { RUST_LOG = "debug" }
//! stop_on_entry = false
//! cargo = ["build", "--bin", "server"]  # build first; its executable is the program
//! build = "make debug"           # or any command run before launching
//! module = "pytest"              # Python: run a module instead of `program`
//! pid = 4242                     # attach: the process id
//!
//! [configuration.extra]          # merged into the adapter's launch arguments
//! justMyCode = false
//!
//! [configuration.adapter_command]  # an adapter Arcode does not know
//! command = "my-dap"
//! args = ["--stdio"]
//! transport = "stdio"            # or "tcp" (`{port}` in args is replaced)
//! ```
//!
//! Variables: `${workspaceFolder}`, `${workspaceFolderBasename}`,
//! `${file}`, `${fileDirname}`, `${fileBasename}`,
//! `${fileBasenameNoExtension}` and `${env:NAME}`. Relative `program` and
//! `cwd` paths are relative to the project folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::adapters::{KnownAdapter, LaunchOptions};
use crate::config::{
    AdapterCommand, AdapterSpec, AdapterTransport, DEFAULT_CONNECT_TIMEOUT, DebugConfig,
    DebugRequest,
};
pub use crate::launch_vars::LaunchVars;

/// Where the file lives inside a project.
pub const LAUNCH_FILE: &str = ".arcode/launch.toml";

/// Which adapter a configuration uses.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterChoice {
    /// A known adapter by name.
    Known(KnownAdapter),
    /// The first installed adapter for a language id.
    Language(String),
    /// A user-defined adapter process.
    Custom(AdapterSpec),
}

/// One `[[configuration]]`.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchEntry {
    /// Display name.
    pub name: String,
    /// The adapter.
    pub adapter: AdapterChoice,
    /// Launch or attach.
    pub request: DebugRequest,
    /// Program (variables not yet substituted).
    pub program: Option<String>,
    /// Python module to run instead of `program`.
    pub module: Option<String>,
    /// Program arguments.
    pub args: Vec<String>,
    /// Working directory (default: the project folder).
    pub cwd: Option<String>,
    /// Extra environment variables.
    pub env: BTreeMap<String, String>,
    /// Stop at the entry point.
    pub stop_on_entry: bool,
    /// `cargo` arguments that build the program first (its executable is
    /// launched; `program` is ignored).
    pub cargo: Option<Vec<String>>,
    /// A command line run before launching.
    pub build: Option<String>,
    /// Process id for `attach`.
    pub pid: Option<u32>,
    /// Extra adapter-specific launch arguments.
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    adapter: Option<String>,
    language: Option<String>,
    request: Option<String>,
    program: Option<String>,
    module: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    stop_on_entry: bool,
    cargo: Option<Vec<String>>,
    build: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    extra: toml::Table,
    adapter_command: Option<RawAdapter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapter {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    transport: Option<String>,
    port: Option<u16>,
    adapter_id: Option<String>,
}

/// Parses the file. Broken entries are skipped with a warning naming them;
/// a syntax error gives no entries and one warning.
pub fn parse_launch_file(text: &str) -> (Vec<LaunchEntry>, Vec<String>) {
    let mut warnings = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => return (Vec::new(), vec![format!("launch.toml: {}", e.message())]),
    };
    let mut entries = Vec::new();
    for (key, value) in table {
        if key != "configuration" {
            warnings.push(format!("launch.toml: unknown key `{key}`"));
            continue;
        }
        let Some(list) = value.as_array() else {
            warnings.push("launch.toml: use [[configuration]] tables".into());
            continue;
        };
        for (ix, item) in list.iter().enumerate() {
            let raw: Result<RawEntry, _> = item.clone().try_into();
            match raw.map_err(|e| e.message().to_string()).and_then(convert) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    let name = item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map_or_else(|| format!("#{}", ix + 1), |n| format!("`{n}`"));
                    warnings.push(format!("launch.toml: configuration {name}: {e}"));
                }
            }
        }
    }
    (entries, warnings)
}

fn convert(raw: RawEntry) -> Result<LaunchEntry, String> {
    let adapter = match (raw.adapter_command, raw.adapter, raw.language) {
        (Some(custom), _, _) => AdapterChoice::Custom(custom_spec(&raw.name, custom)?),
        (None, Some(name), _) => AdapterChoice::Known(
            KnownAdapter::from_name(&name).ok_or_else(|| format!("unknown adapter `{name}`"))?,
        ),
        (None, None, Some(language)) => AdapterChoice::Language(language.to_ascii_lowercase()),
        (None, None, None) => AdapterChoice::Language(
            infer_language(raw.program.as_deref(), raw.module.is_some()).to_string(),
        ),
    };
    let request = match raw.request.as_deref().unwrap_or("launch") {
        "launch" => DebugRequest::Launch,
        "attach" => DebugRequest::Attach,
        other => return Err(format!("request must be launch or attach, not `{other}`")),
    };
    let extra = serde_json::to_value(raw.extra)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if request == DebugRequest::Launch
        && raw.program.is_none()
        && raw.module.is_none()
        && raw.cargo.is_none()
        && !extra.contains_key("program")
    {
        return Err("set `program`, `module` or `cargo`".into());
    }
    Ok(LaunchEntry {
        name: raw.name,
        adapter,
        request,
        program: raw.program,
        module: raw.module,
        args: raw.args,
        cwd: raw.cwd,
        env: raw.env,
        stop_on_entry: raw.stop_on_entry,
        cargo: raw.cargo,
        build: raw.build,
        pid: raw.pid,
        extra,
    })
}

fn infer_language(program: Option<&str>, module: bool) -> &'static str {
    let ext = program
        .and_then(|p| Path::new(p).extension())
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    match ext {
        _ if module => "python",
        "py" => "python",
        "js" | "mjs" | "cjs" | "ts" => "javascript",
        _ => "rust",
    }
}

fn custom_spec(name: &str, raw: RawAdapter) -> Result<AdapterSpec, String> {
    let env: Vec<(String, String)> = raw.env.into_iter().collect();
    let transport = match raw.transport.as_deref().unwrap_or("stdio") {
        "stdio" => AdapterTransport::Stdio {
            command: raw.command,
            args: raw.args,
            env,
            cwd: None,
        },
        "tcp" => AdapterTransport::Tcp {
            command: Some(AdapterCommand {
                command: raw.command,
                args: raw.args,
                env,
                cwd: None,
            }),
            host: "127.0.0.1".into(),
            port: raw.port.unwrap_or(0),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        },
        other => return Err(format!("transport must be stdio or tcp, not `{other}`")),
    };
    Ok(AdapterSpec {
        name: name.to_string(),
        adapter_id: raw.adapter_id.unwrap_or_else(|| name.to_string()),
        transport,
        install_hint: Some("check `adapter_command` in .arcode/launch.toml".into()),
        configure_before_launch: false,
    })
}

impl LaunchEntry {
    /// The resolved working directory.
    pub fn cwd(&self, vars: &LaunchVars) -> PathBuf {
        self.cwd
            .as_deref()
            .map_or_else(|| vars.workspace_folder.clone(), |c| vars.path(c))
    }

    /// Builds the session config for `adapter` (resolved by the caller from
    /// [`LaunchEntry::adapter`]). `program` overrides the entry's program
    /// (the executable a `cargo` build produced).
    pub fn to_config(
        &self,
        spec: AdapterSpec,
        known: Option<KnownAdapter>,
        vars: &LaunchVars,
        program: Option<PathBuf>,
    ) -> DebugConfig {
        let program = program.or_else(|| self.program.as_deref().map(|p| vars.path(p)));
        let options = LaunchOptions {
            program: program.clone().unwrap_or_default(),
            args: self.args.iter().map(|a| vars.substitute(a)).collect(),
            cwd: self.cwd(vars),
            env: self
                .env
                .iter()
                .map(|(k, v)| (k.clone(), vars.substitute(v)))
                .collect(),
            stop_on_entry: self.stop_on_entry,
        };
        let mut arguments = match (self.request, known) {
            (DebugRequest::Launch, Some(adapter)) => adapter.launch_arguments(&options),
            (DebugRequest::Launch, None) => json!({
                "program": program.map(|p| p.to_string_lossy().into_owned()),
                "args": options.args, "cwd": options.cwd.to_string_lossy(),
                "stopOnEntry": options.stop_on_entry,
            }),
            (DebugRequest::Attach, _) => json!({}),
        };
        if let Value::Object(map) = &mut arguments {
            if let Some(module) = &self.module {
                map.remove("program");
                map.insert("module".into(), json!(vars.substitute(module)));
            }
            if self.request == DebugRequest::Attach {
                if let Some(pid) = self.pid {
                    // debugpy and Delve read `processId`; the LLDB and
                    // GDB adapters read `pid`.
                    let key = match known {
                        Some(KnownAdapter::Debugpy | KnownAdapter::Delve) => "processId",
                        _ => "pid",
                    };
                    map.insert(key.into(), json!(pid));
                }
                if known == Some(KnownAdapter::Delve) {
                    map.insert("mode".into(), json!("local"));
                }
            }
            for (k, v) in &self.extra {
                map.insert(k.clone(), substitute_json(v, vars));
            }
        }
        DebugConfig::new(spec, self.request, arguments)
    }
}

fn substitute_json(value: &Value, vars: &LaunchVars) -> Value {
    match value {
        Value::String(s) => Value::String(vars.substitute(s)),
        Value::Array(a) => Value::Array(a.iter().map(|v| substitute_json(v, vars)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), substitute_json(v, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
#[path = "launch_file_tests.rs"]
mod tests;
