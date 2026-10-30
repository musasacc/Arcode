//! Run commands from `Cargo.toml`.

use std::path::Path;

use crate::cargo::{self, CargoPackage};
use crate::command::{RunCommand, RunKind, RunSource, args};

/// `cargo run` per binary, then `cargo test` and `cargo build`.
///
/// A single package with one binary (or a `default-run`) gets a plain
/// `cargo run`; other binaries get `--bin <name>`. In a workspace every
/// binary gets `cargo run -p <package> --bin <bin>`, and test/build use
/// `--workspace`.
pub(super) fn detect(root: &Path) -> Vec<RunCommand> {
    let Some(project) = cargo::read_project(root) else {
        return Vec::new();
    };
    let cmd =
        |list: Vec<String>, kind| RunCommand::new("cargo", list, root, kind, RunSource::Cargo);
    let mut out = Vec::new();
    if project.is_workspace {
        for pkg in &project.packages {
            for (bin, _) in &pkg.bins {
                out.push(cmd(
                    args(&["run", "-p", &pkg.name, "--bin", bin]),
                    RunKind::Run,
                ));
            }
        }
        out.push(cmd(args(&["test", "--workspace"]), RunKind::Test));
        out.push(cmd(args(&["build", "--workspace"]), RunKind::Build));
    } else if let Some(pkg) = project.packages.first() {
        out.extend(
            single_package_runs(pkg)
                .into_iter()
                .map(|a| cmd(a, RunKind::Run)),
        );
        out.push(cmd(args(&["test"]), RunKind::Test));
        out.push(cmd(args(&["build"]), RunKind::Build));
    }
    out
}

fn single_package_runs(pkg: &CargoPackage) -> Vec<Vec<String>> {
    let mut runs = Vec::new();
    let default = match (&pkg.default_run, pkg.bins.as_slice()) {
        (Some(d), _) => Some(d.as_str()),
        (None, [(only, _)]) => Some(only.as_str()),
        _ => None,
    };
    if default.is_some() {
        runs.push(args(&["run"]));
    }
    for (bin, _) in &pkg.bins {
        if Some(bin.as_str()) != default {
            runs.push(args(&["run", "--bin", bin]));
        }
    }
    runs
}
