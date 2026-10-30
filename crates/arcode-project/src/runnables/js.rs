//! Commands for JavaScript/TypeScript runnables (vitest, jest, node --test).

use std::path::Path;

use serde_json::Value;

use super::go::regex_escape;
use super::{RunnableKind, label};
use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::fsutil;
use crate::node::{self, NodePackageManager};

/// The test runner a package uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framework {
    Vitest,
    Jest,
    Bun,
    NodeTest,
}

pub(super) fn command(
    file: &Path,
    root: &Path,
    kind: RunnableKind,
    name: &str,
) -> Option<RunCommand> {
    let dir = fsutil::start_dir(file);
    let pkg_dir = node::nearest_package_dir(&dir, Some(root)).unwrap_or_else(|| root.to_path_buf());
    let json = fsutil::read_json(&pkg_dir.join("package.json")).unwrap_or(Value::Null);
    let (pm, pm_dir) = node::node_package_manager(&pkg_dir, Some(root));
    // Monorepos usually declare test runners in the workspace root only.
    let workspace_json = (pm_dir != pkg_dir)
        .then(|| fsutil::read_json(&pm_dir.join("package.json")))
        .flatten();
    let manifests: Vec<&Value> = std::iter::once(&json).chain(&workspace_json).collect();
    let rel = fsutil::rel_slash(file, &pkg_dir);
    let (program, list, run_kind) = if kind == RunnableKind::Main {
        let (program, list) = main_args(&manifests, pm, &rel);
        (program, list, RunKind::Run)
    } else {
        let (program, list) = test_args(&manifests, pm, &rel, kind, name)?;
        (program, list, RunKind::Test)
    };
    let shown = if kind == RunnableKind::Main { "" } else { name };
    Some(
        RunCommand::new(program, list, &pkg_dir, run_kind, RunSource::Node)
            .with_label(label(kind, shown)),
    )
}

/// Program and arguments for a test, test group or benchmark.
fn test_args(
    manifests: &[&Value],
    pm: NodePackageManager,
    rel: &str,
    kind: RunnableKind,
    name: &str,
) -> Option<(String, Vec<String>)> {
    let pattern = regex_escape(name);
    let filter = |flag: &str| -> Vec<String> {
        if name.is_empty() {
            Vec::new()
        } else {
            vec![flag.to_string(), pattern.clone()]
        }
    };
    let bench = kind == RunnableKind::Benchmark;
    let exec = |tail: Vec<String>| {
        let (program, mut list) = pm.exec_prefix();
        list.extend(tail);
        (program.to_string(), list)
    };
    let result = match framework(manifests, pm) {
        Framework::Vitest => {
            let mut tail = if bench {
                args(&["vitest", "bench", "--run"])
            } else {
                args(&["vitest", "run"])
            };
            tail.push(rel.to_string());
            tail.extend(filter("-t"));
            exec(tail)
        }
        _ if bench => return None,
        Framework::Jest => {
            let mut tail = args(&["jest", rel]);
            tail.extend(filter("-t"));
            exec(tail)
        }
        Framework::Bun => {
            let mut list = args(&["test", rel]);
            list.extend(filter("-t"));
            ("bun".to_string(), list)
        }
        Framework::NodeTest => {
            let mut list = args(&["--test"]);
            list.extend(filter("--test-name-pattern"));
            list.push(rel.to_string());
            ("node".to_string(), list)
        }
    };
    Some(result)
}

/// `node <file>`; for TypeScript, `tsx`/`ts-node` when the package depends
/// on one (modern Node runs `.ts` files directly otherwise); `bun <file>` in
/// bun projects.
fn main_args(manifests: &[&Value], pm: NodePackageManager, rel: &str) -> (String, Vec<String>) {
    let is_ts = [".ts", ".mts", ".cts", ".tsx"]
        .iter()
        .any(|e| rel.ends_with(e));
    let runner = ["tsx", "ts-node"]
        .into_iter()
        .find(|r| is_ts && depends_on(manifests, r));
    match runner {
        Some(runner) => {
            let (program, mut list) = pm.exec_prefix();
            list.extend(args(&[runner, rel]));
            (program.to_string(), list)
        }
        None if pm == NodePackageManager::Bun => ("bun".to_string(), args(&[rel])),
        None => ("node".to_string(), args(&[rel])),
    }
}

/// Whether the package or its workspace root depends on `name`.
fn depends_on(manifests: &[&Value], name: &str) -> bool {
    manifests
        .iter()
        .any(|json| node::has_dependency(json, name))
}

/// The package's own runner wins over one declared in the workspace root.
fn framework(manifests: &[&Value], pm: NodePackageManager) -> Framework {
    let found = |name| {
        manifests
            .iter()
            .position(|json| node::has_dependency(json, name))
    };
    match (found("vitest"), found("jest")) {
        (Some(vitest), Some(jest)) if jest < vitest => Framework::Jest,
        (Some(_), _) => Framework::Vitest,
        (None, Some(_)) => Framework::Jest,
        (None, None) if pm == NodePackageManager::Bun => Framework::Bun,
        (None, None) => Framework::NodeTest,
    }
}
