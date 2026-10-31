//! Which debug adapters are installed, and what to tell the user when none
//! is.
//!
//! Nothing is installed or downloaded here. [`locate_adapter`] looks the
//! adapter's executable up on `PATH` (trying versioned names such as
//! `lldb-dap-18` that distributions install), checks what can be checked
//! offline (GDB's version, Python's `debugpy` module) and returns a spec
//! that starts the executable it found. These checks spawn short-lived
//! processes: call them off the UI thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::adapters::{KnownAdapter, adapters_for_language};
use crate::config::{AdapterSpec, AdapterTransport};
use crate::error::{DapError, Result};
use crate::resolve::find_executable;

/// Oldest GDB with a DAP interpreter.
pub const MIN_GDB_MAJOR: u32 = 14;

impl KnownAdapter {
    /// Looks an adapter up by its display name or executable name
    /// (`"lldb-dap"`, `"codelldb"`, `"gdb"`, `"debugpy"`, `"delve"` /
    /// `"dlv"`, `"js-debug"`), ignoring case.
    pub fn from_name(name: &str) -> Option<KnownAdapter> {
        let name = name.trim().to_ascii_lowercase();
        KnownAdapter::ALL.into_iter().find(|a| {
            a.name() == name
                || a.executable() == name
                || (*a == KnownAdapter::LldbDap && name == "lldb-vscode")
        })
    }

    /// Executable names tried in order. Distributions often install LLVM
    /// tools with a version suffix only (`lldb-dap-18`), and older LLVM
    /// releases call the adapter `lldb-vscode`.
    pub fn executable_names(self) -> Vec<String> {
        match self {
            KnownAdapter::LldbDap => {
                let mut names = vec!["lldb-dap".to_string(), "lldb-vscode".to_string()];
                for version in (14..=24).rev() {
                    names.push(format!("lldb-dap-{version}"));
                    names.push(format!("lldb-vscode-{version}"));
                }
                names
            }
            KnownAdapter::Debugpy if !cfg!(windows) => {
                vec!["python3".to_string(), "python".to_string()]
            }
            other => vec![other.executable().to_string()],
        }
    }

    /// What to install, in words, e.g. "GDB 14 or newer (`gdb`)".
    pub fn install_help(self) -> &'static str {
        match self {
            KnownAdapter::CodeLldb => {
                "CodeLLDB's adapter (`codelldb`, from the vscode-lldb releases) on PATH"
            }
            KnownAdapter::LldbDap => "LLVM's LLDB, which provides `lldb-dap` (`lldb-vscode`)",
            KnownAdapter::Gdb => "GDB 14 or newer (`gdb`)",
            KnownAdapter::Debugpy => "debugpy for Python (`python3 -m pip install debugpy`)",
            KnownAdapter::Delve => "Delve (`go install github.com/go-delve/delve/cmd/dlv@latest`)",
            KnownAdapter::JsDebug => "vscode-js-debug's DAP server (`dapDebugServer.js`)",
        }
    }
}

/// A spec that starts `executable` instead of the adapter's bare name.
pub fn spec_with_executable(adapter: KnownAdapter, executable: &Path) -> AdapterSpec {
    let mut spec = adapter.spec();
    let path = executable.to_string_lossy().into_owned();
    match &mut spec.transport {
        AdapterTransport::Stdio { command, .. } => *command = path,
        AdapterTransport::Tcp {
            command: Some(command),
            ..
        } => command.command = path,
        AdapterTransport::Tcp { command: None, .. } => {}
    }
    spec
}

/// Finds an installed adapter executable. `configured` is a path the user
/// set for this adapter (used as is when it exists). Returns `None` when
/// the adapter is missing or unusable (GDB older than 14, Python without
/// `debugpy`). Blocking.
pub fn locate_adapter(adapter: KnownAdapter, configured: Option<&Path>) -> Option<PathBuf> {
    if !adapter.is_supported() {
        return None;
    }
    let found = match configured.filter(|p| !p.as_os_str().is_empty()) {
        Some(path) => find_executable(&path.to_string_lossy(), &[]),
        None => adapter
            .executable_names()
            .iter()
            .find_map(|name| find_executable(name, &[])),
    }?;
    let usable = match adapter {
        KnownAdapter::Gdb => gdb_major(&found).is_some_and(|v| v >= MIN_GDB_MAJOR),
        KnownAdapter::Debugpy => succeeds(&found, &["-c", "import debugpy"]),
        _ => true,
    };
    usable.then_some(found)
}

/// The first usable adapter for `language_id`, trying `preferred` first
/// (an adapter name from the settings, empty for none), then the language's
/// adapters in their default order. `configured(adapter)` gives a path the
/// user set for an adapter. Blocking.
pub fn detect_adapter(
    language_id: &str,
    preferred: &str,
    configured: &dyn Fn(KnownAdapter) -> Option<PathBuf>,
) -> Result<(KnownAdapter, AdapterSpec)> {
    let mut order: Vec<KnownAdapter> = Vec::new();
    if let Some(first) = KnownAdapter::from_name(preferred) {
        order.push(first);
    }
    for adapter in adapters_for_language(language_id) {
        if !order.contains(adapter) {
            order.push(*adapter);
        }
    }
    let supported = order.iter().any(|a| a.is_supported());
    for adapter in order.iter().copied().filter(|a| a.is_supported()) {
        if let Some(path) = locate_adapter(adapter, configured(adapter).as_deref()) {
            return Ok((adapter, spec_with_executable(adapter, &path)));
        }
    }
    if !supported {
        return Err(DapError::Unsupported(missing_adapter_message(language_id)));
    }
    Err(DapError::NotFound(missing_adapter_message(language_id)))
}

/// A message naming what to install to debug `language_id`, e.g. "No
/// debug adapter for rust was found. Install one of: ...". Nothing is
/// installed automatically.
pub fn missing_adapter_message(language_id: &str) -> String {
    let adapters = adapters_for_language(language_id);
    if adapters.is_empty() {
        return format!("Debugging {language_id} files is not supported.");
    }
    if adapters.iter().all(|a| !a.is_supported()) {
        let help: Vec<&str> = adapters.iter().map(|a| a.install_help()).collect();
        return format!(
            "Debugging {language_id} needs {}, and Arcode cannot drive it yet \
             (it needs child debug sessions).",
            help.join(" or ")
        );
    }
    let help: Vec<&str> = adapters
        .iter()
        .filter(|a| a.is_supported())
        .map(|a| a.install_help())
        .collect();
    let list = if help.len() == 1 {
        help[0].to_string()
    } else {
        format!("one of: {}", help.join("; "))
    };
    format!(
        "No debug adapter for {language_id} was found. Install {list}. \
         Nothing is downloaded automatically."
    )
}

/// GDB's major version from `gdb --version`.
fn gdb_major(gdb: &Path) -> Option<u32> {
    let out = Command::new(gdb)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_gdb_major(&String::from_utf8_lossy(&out.stdout))
}

/// Parses the major version from the first line of `gdb --version`, e.g.
/// `GNU gdb (Ubuntu 15.1-1ubuntu1~24.04.1) 15.1` or `GNU gdb (GDB) 14.2`.
pub(crate) fn parse_gdb_major(text: &str) -> Option<u32> {
    let line = text.lines().next()?;
    let last = line.split_whitespace().last()?;
    last.split('.').next()?.parse().ok()
}

fn succeeds(program: &Path, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_adapters() {
        assert_eq!(KnownAdapter::from_name("GDB"), Some(KnownAdapter::Gdb));
        assert_eq!(KnownAdapter::from_name("dlv"), Some(KnownAdapter::Delve));
        assert_eq!(
            KnownAdapter::from_name("lldb-vscode"),
            Some(KnownAdapter::LldbDap)
        );
        assert_eq!(KnownAdapter::from_name("nope"), None);
        assert_eq!(KnownAdapter::from_name(""), None);
    }

    #[test]
    fn versioned_lldb_names_are_tried_after_the_plain_ones() {
        let names = KnownAdapter::LldbDap.executable_names();
        assert_eq!(names[0], "lldb-dap");
        assert!(names.contains(&"lldb-dap-18".to_string()));
        assert_eq!(KnownAdapter::Gdb.executable_names(), ["gdb"]);
    }

    #[test]
    fn gdb_versions_parse() {
        assert_eq!(
            parse_gdb_major("GNU gdb (Ubuntu 15.1-1ubuntu1~24.04.1) 15.1\nCopyright"),
            Some(15)
        );
        assert_eq!(parse_gdb_major("GNU gdb (GDB) 14.2"), Some(14));
        assert_eq!(parse_gdb_major("GNU gdb 12.1"), Some(12));
        assert_eq!(parse_gdb_major(""), None);
    }

    #[test]
    fn executable_override_keeps_the_transport() {
        let spec = spec_with_executable(KnownAdapter::CodeLldb, Path::new("/opt/x/codelldb"));
        let AdapterTransport::Tcp {
            command: Some(c), ..
        } = spec.transport
        else {
            panic!("tcp expected");
        };
        assert_eq!(c.command, "/opt/x/codelldb");
        let spec = spec_with_executable(KnownAdapter::LldbDap, Path::new("/usr/bin/lldb-dap-18"));
        assert_eq!(spec.executable(), Some("/usr/bin/lldb-dap-18"));
    }

    #[test]
    fn missing_messages_say_what_to_install() {
        let rust = missing_adapter_message("rust");
        assert!(rust.contains("lldb-dap"), "{rust}");
        assert!(rust.contains("GDB 14"), "{rust}");
        assert!(rust.contains("Nothing is downloaded"), "{rust}");
        let py = missing_adapter_message("python");
        assert!(py.contains("pip install debugpy"), "{py}");
        assert!(missing_adapter_message("typescript").contains("cannot drive it yet"));
        assert!(missing_adapter_message("cobol").contains("not supported"));
    }

    #[test]
    fn detection_reports_missing_adapters() {
        let none = |_| Some(PathBuf::from("/definitely/not/here"));
        let err = detect_adapter("python", "", &none).unwrap_err();
        assert!(matches!(err, DapError::NotFound(_)), "{err:?}");
        assert!(err.to_string().contains("debugpy"));
        let err = detect_adapter("javascript", "", &none).unwrap_err();
        assert!(matches!(err, DapError::Unsupported(_)));
    }
}
