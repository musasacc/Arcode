//! Launch configurations offered in the Run and Debug view: the project's
//! `.arcode/launch.toml` entries first, then ones generated from the
//! project's run commands (Cargo binaries, a Python `main.py`, Go
//! packages). Blocking file reads: call [`load_choices`] off the UI thread.

use std::path::Path;

use arcode_dap::{LAUNCH_FILE, LaunchEntry, parse_launch_file};
use arcode_project::{DebugTarget, RunCommand, RunKind, debug_target, detect_run_commands};

/// What a launch configuration starts.
#[derive(Debug, Clone, PartialEq)]
pub enum LaunchKind {
    /// An entry of `.arcode/launch.toml`.
    File(Box<LaunchEntry>),
    /// A run command or runnable (generated).
    Target(Box<DebugTarget>),
}

/// A launch configuration the user can pick.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchChoice {
    /// Display name.
    pub name: String,
    /// What it starts.
    pub kind: LaunchKind,
}

impl LaunchChoice {
    /// A generated configuration for a run or test command.
    pub fn for_command(command: &RunCommand) -> Option<Self> {
        Some(LaunchChoice {
            name: command.label.clone(),
            kind: LaunchKind::Target(Box::new(debug_target(command)?)),
        })
    }

    /// Whether it comes from `launch.toml`.
    pub fn is_from_file(&self) -> bool {
        matches!(self.kind, LaunchKind::File(_))
    }
}

/// The configurations of the project in `root`, and warnings about
/// `launch.toml`.
pub fn load_choices(root: &Path) -> (Vec<LaunchChoice>, Vec<String>) {
    let mut choices = Vec::new();
    let mut warnings = Vec::new();
    if let Ok(text) = std::fs::read_to_string(root.join(LAUNCH_FILE)) {
        let (entries, file_warnings) = parse_launch_file(&text);
        warnings = file_warnings;
        choices.extend(entries.into_iter().map(|entry| LaunchChoice {
            name: entry.name.clone(),
            kind: LaunchKind::File(Box::new(entry)),
        }));
    }
    choices.extend(generated(&detect_run_commands(root)));
    (choices, warnings)
}

/// Debuggable run commands (programs, not tests or builds).
fn generated(commands: &[RunCommand]) -> Vec<LaunchChoice> {
    commands
        .iter()
        .filter(|c| c.kind == RunKind::Run)
        .filter_map(LaunchChoice::for_command)
        .collect()
}

/// The starter `launch.toml` written by "Debug: Open launch.toml" when the
/// project has none.
pub fn template(first_program: Option<&str>) -> String {
    let program = first_program.unwrap_or("target/debug/app");
    format!(
        r#"# Launch configurations for the Run and Debug view.
# Docs: each [[configuration]] has a name and either `adapter`
# (codelldb, lldb-dap, gdb, debugpy, delve) or `language` (the first
# installed adapter for it). Arcode never downloads adapters.
#
# Variables: ${{workspaceFolder}}, ${{file}}, ${{fileDirname}},
# ${{fileBasenameNoExtension}}, ${{env:NAME}}.

[[configuration]]
name = "Debug program"
language = "rust"
# Build with cargo first and debug the executable it produces:
# cargo = ["build", "--bin", "app"]
program = "{program}"
args = []
cwd = "${{workspaceFolder}}"
# env = {{ RUST_LOG = "debug" }}
# stop_on_entry = false
# build = "make debug"        # a command run before launching

# [[configuration]]
# name = "Python file"
# language = "python"
# program = "${{file}}"
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_entries_come_before_generated_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(root.join(".arcode")).unwrap();
        std::fs::write(
            root.join(LAUNCH_FILE),
            "[[configuration]]\nname = \"Mine\"\nprogram = \"x.py\"\n\n[[configuration]]\nname = \"bad\"\n",
        )
        .unwrap();
        let (choices, warnings) = load_choices(root);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(choices[0].name, "Mine");
        assert!(choices[0].is_from_file());
        let generated = &choices[1];
        assert!(!generated.is_from_file());
        let LaunchKind::Target(target) = &generated.kind else {
            panic!("generated target expected");
        };
        assert_eq!(target.language, "rust");
        assert!(
            target
                .build
                .as_ref()
                .unwrap()
                .args
                .contains(&"build".to_string())
        );
    }

    #[test]
    fn template_parses() {
        let (entries, warnings) = parse_launch_file(&template(None));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 1);
    }
}
