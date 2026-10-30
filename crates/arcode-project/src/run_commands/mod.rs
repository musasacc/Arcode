//! Detects the run, test and build commands a project offers.

mod cargo_cmds;
mod go_cmds;
mod make;
mod node_cmds;
mod python_cmds;

use std::path::Path;

use crate::command::{RunCommand, RunKind, RunSource};

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
///
/// Housekeeping and lifecycle scripts, targets and recipes (`clean`,
/// `install`, `deploy`, `publish`, `lint`, `format`, `prepare`, npm
/// `pre`/`post` hooks, ...) are listed by [`detect_run_commands`] but never
/// chosen here; `None` when nothing else is left.
pub fn default_run_command(root: &Path) -> Option<RunCommand> {
    let mut candidates: Vec<RunCommand> = detect_run_commands(root)
        .into_iter()
        .filter(|c| !is_housekeeping_command(c))
        .collect();
    let index = candidates
        .iter()
        .position(|c| c.kind == RunKind::Run)
        .unwrap_or(0);
    (index < candidates.len()).then(|| candidates.swap_remove(index))
}

/// Whether a script/target/recipe command runs a housekeeping name.
fn is_housekeeping_command(command: &RunCommand) -> bool {
    let named = matches!(
        command.source,
        RunSource::Node | RunSource::Make | RunSource::Just
    );
    named
        && command
            .args
            .last()
            .is_some_and(|name| is_housekeeping(name))
}

/// Names that clean, install, publish, deploy, lint or format instead of
/// running the program, including npm lifecycle hooks (`prestart`,
/// `postinstall`, ...).
pub(crate) fn is_housekeeping(name: &str) -> bool {
    const HOUSEKEEPING: [&str; 24] = [
        "clean",
        "distclean",
        "mrproper",
        "clobber",
        "purge",
        "reset",
        "install",
        "uninstall",
        "deploy",
        "publish",
        "upload",
        "lint",
        "format",
        "fmt",
        "prettier",
        "prepare",
        "prepublish",
        "prepublishOnly",
        "prepack",
        "postpack",
        "version",
        "postversion",
        "help",
        "setup",
    ];
    const LIFECYCLE: [&str; 9] = [
        "install", "start", "test", "build", "publish", "pack", "version", "restart", "stop",
    ];
    let base = name.split([':', '-', '_']).next().unwrap_or(name);
    if HOUSEKEEPING.contains(&base) {
        return true;
    }
    ["pre", "post"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| LIFECYCLE.contains(&rest))
    })
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

    #[test]
    fn housekeeping_names() {
        for name in [
            "clean",
            "clean:dist",
            "distclean",
            "install",
            "uninstall",
            "deploy",
            "publish",
            "lint",
            "lint-fix",
            "format",
            "prepare",
            "postinstall",
            "prestart",
            "pretest",
        ] {
            assert!(is_housekeeping(name), "{name}");
        }
        for name in [
            "start", "dev", "serve", "preview", "postcss", "run", "app", "watch",
        ] {
            assert!(!is_housekeeping(name), "{name}");
        }
    }
}
