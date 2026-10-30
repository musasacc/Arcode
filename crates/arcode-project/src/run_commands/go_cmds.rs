//! Run commands for Go modules.

use std::fs;
use std::path::Path;

use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::{fsutil, gofile};

/// `go run .` (if the root is a `main` package), `go run ./cmd/<x>` for each
/// main package under `cmd/`, then `go test ./...` and `go build ./...`.
pub(super) fn detect(root: &Path) -> Vec<RunCommand> {
    if !root.join("go.mod").is_file() {
        return Vec::new();
    }
    let cmd = |list: Vec<String>, kind| RunCommand::new("go", list, root, kind, RunSource::Go);
    let mut out = Vec::new();
    if is_main_package(root) {
        out.push(cmd(args(&["run", "."]), RunKind::Run));
    }
    let cmd_dir = root.join("cmd");
    for name in fsutil::list_dir(&cmd_dir, |p| p.is_dir() && is_main_package(p)) {
        out.push(cmd(args(&["run", &format!("./cmd/{name}")]), RunKind::Run));
    }
    out.push(cmd(args(&["test", "./..."]), RunKind::Test));
    out.push(cmd(args(&["build", "./..."]), RunKind::Build));
    out
}

/// Whether any non-test `.go` file in `dir` declares `package main`,
/// ignoring files excluded by an `ignore` build constraint (`go:generate`
/// helpers), which `go run .` does not see.
pub(crate) fn is_main_package(dir: &Path) -> bool {
    let files = fsutil::list_dir(dir, |p| p.is_file());
    files
        .iter()
        .filter(|n| n.ends_with(".go") && !n.ends_with("_test.go"))
        .any(|n| {
            fs::read_to_string(dir.join(n)).is_ok_and(|text| {
                gofile::package_name(&text) == Some("main") && !gofile::is_build_ignored(&text)
            })
        })
}
