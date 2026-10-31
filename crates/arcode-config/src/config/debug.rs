//! The `[debug]` section of `config.toml`: run buttons and debug adapters.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[debug]`: the debugger and the run buttons. Adapters are never
/// installed or downloaded; Arcode uses what is on `PATH` (or a path set
/// here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugSettings {
    /// Show run buttons next to `main` functions and tests in the gutter.
    pub run_buttons: bool,
    /// Show the Debug panel when a debug session starts.
    pub open_panel_on_start: bool,
    /// Preferred adapter per language id, e.g. `rust = "gdb"` (names:
    /// `codelldb`, `lldb-dap`, `gdb`, `debugpy`, `delve`).
    pub adapters: BTreeMap<String, String>,
    /// Executable per adapter name when it is not on `PATH`, e.g.
    /// `"lldb-dap" = "/usr/bin/lldb-dap-18"`.
    pub adapter_paths: BTreeMap<String, String>,
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self {
            run_buttons: true,
            open_panel_on_start: true,
            adapters: BTreeMap::new(),
            adapter_paths: BTreeMap::new(),
        }
    }
}

impl_section!(DebugSettings {
    run_buttons,
    open_panel_on_start,
    adapters,
    adapter_paths,
});

impl DebugSettings {
    /// The preferred adapter name for a language (empty for none).
    pub fn adapter_for(&self, language_id: &str) -> &str {
        self.adapters.get(language_id).map_or("", String::as_str)
    }

    /// The configured executable of an adapter, if any.
    pub fn adapter_path(&self, adapter: &str) -> Option<&str> {
        self.adapter_paths
            .get(adapter)
            .map(String::as_str)
            .filter(|p| !p.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn debug_section_parses() {
        let (config, warnings) = Config::from_toml_str(
            "[debug]\nrun_buttons = false\nadapters = { rust = \"gdb\" }\n\
             adapter_paths = { \"lldb-dap\" = \"/usr/bin/lldb-dap-18\" }\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!config.debug.run_buttons);
        assert!(config.debug.open_panel_on_start);
        assert_eq!(config.debug.adapter_for("rust"), "gdb");
        assert_eq!(config.debug.adapter_for("python"), "");
        assert_eq!(
            config.debug.adapter_path("lldb-dap"),
            Some("/usr/bin/lldb-dap-18")
        );
        let (_, warnings) = Config::from_toml_str("[debug]\nrun_buttons = 3\n");
        assert_eq!(warnings.len(), 1);
    }
}
