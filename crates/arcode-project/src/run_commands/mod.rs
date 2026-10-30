//! Detects the run, test and build commands a project offers.

mod cargo_cmds;
mod go_cmds;
mod make;
mod node_cmds;
mod python_cmds;

use std::path::Path;

use crate::command::{RunCommand, RunKind};

/// All run, test and build commands found in `root`, best first.
///
/// Sources, in order: Cargo, `package.json` scripts, Go, Python, `justfile`
/// recipes, `Makefile` targets. Each source lists its run commands, then
/// tests, then builds. Reads files only; never spawns a process.
pub fn detect_run_commands(root: &Path) -> Vec<RunCommand> {
    let mut commands = Vec::new();
    commands.extend(cargo_cmds::detect(root));
    commands.extend(node_cmds::detect(root));
    commands.extend(go_cmds::detect(root));
    commands.extend(python_cmds::detect(root));
    commands.extend(make::detect_just(root));
    commands.extend(make::detect_make(root));
    commands
}

/// The command a "Run" button should use: the first [`RunKind::Run`]
/// command of [`detect_run_commands`], else the first command of any kind.
pub fn default_run_command(root: &Path) -> Option<RunCommand> {
    let commands = detect_run_commands(root);
    let index = commands
        .iter()
        .position(|c| c.kind == RunKind::Run)
        .unwrap_or(0);
    commands.into_iter().nth(index)
}

/// Classifies a script, target or recipe name.
pub(crate) fn kind_for_name(name: &str) -> RunKind {
    let base = name.split([':', '-', '_']).next().unwrap_or(name);
    match base {
        "test" | "tests" | "check" | "spec" | "e2e" | "coverage" => RunKind::Test,
        "build" | "compile" | "all" | "dist" | "release" | "bundle" => RunKind::Build,
        _ => RunKind::Run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_classified() {
        assert_eq!(kind_for_name("test:unit"), RunKind::Test);
        assert_eq!(kind_for_name("build"), RunKind::Build);
        assert_eq!(kind_for_name("dev"), RunKind::Run);
        assert_eq!(kind_for_name("serve"), RunKind::Run);
    }
}
