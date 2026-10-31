//! Commands for Rust runnables (`cargo test`, `cargo bench`, `cargo run`).

use std::path::Path;

use super::{RunnableKind, label};
use crate::cargo::{self, CargoPackage};
use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::fsutil;

/// The Cargo target a file belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Lib,
    Bin(String),
    Test(String),
    Bench(String),
    Example(String),
    Unknown,
}

impl Target {
    fn selector(&self) -> Vec<String> {
        match self {
            Self::Lib => args(&["--lib"]),
            Self::Bin(n) => args(&["--bin", n]),
            Self::Test(n) => args(&["--test", n]),
            Self::Bench(n) => args(&["--bench", n]),
            Self::Example(n) => args(&["--example", n]),
            Self::Unknown => Vec::new(),
        }
    }
}

pub(super) fn command(
    file: &Path,
    root: &Path,
    kind: RunnableKind,
    name: &str,
) -> Option<RunCommand> {
    let pkg = cargo::package_for(&fsutil::start_dir(file), Some(root))?;
    let rel = fsutil::rel_slash(file, &pkg.dir);
    let (target, module) = locate(&pkg, &rel);
    let path = join_path(&module, name);
    let mut list = Vec::new();
    let run_kind = match kind {
        RunnableKind::Main => {
            list.extend(args(&["run", "--package", &pkg.name]));
            match &target {
                Target::Bin(_) | Target::Example(_) => list.extend(target.selector()),
                _ => {}
            }
            RunKind::Run
        }
        RunnableKind::Test | RunnableKind::TestModule | RunnableKind::Benchmark => {
            let bench = kind == RunnableKind::Benchmark;
            list.extend(args(&[
                if bench { "bench" } else { "test" },
                "--package",
                &pkg.name,
            ]));
            list.extend(target.selector());
            list.push("--".to_string());
            if !path.is_empty() {
                list.push(path.clone());
            }
            if kind != RunnableKind::TestModule && !path.is_empty() {
                list.push("--exact".to_string());
            }
            if !bench {
                list.push("--nocapture".to_string());
            }
            RunKind::Test
        }
    };
    let shown = if kind == RunnableKind::Main {
        String::new()
    } else {
        path
    };
    Some(
        RunCommand::new("cargo", list, root, run_kind, RunSource::Cargo)
            .with_label(label(kind, &shown)),
    )
}

/// `prefix::name`, skipping empty parts.
fn join_path(prefix: &str, name: &str) -> String {
    match (prefix.is_empty(), name.is_empty()) {
        (true, _) => name.to_string(),
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}::{name}"),
    }
}

/// The target and module path for a file at `rel` (relative to the package).
fn locate(pkg: &CargoPackage, rel: &str) -> (Target, String) {
    if let Some(bin) = pkg.bin_for_path(rel) {
        return (Target::Bin(bin.to_string()), String::new());
    }
    let parts: Vec<&str> = rel.split('/').collect();
    let stem = |s: &str| s.trim_end_matches(".rs").to_string();
    match parts.as_slice() {
        ["src", "lib.rs"] => (Target::Lib, String::new()),
        ["src", "bin", bin, rest @ ..] if !rest.is_empty() => {
            (Target::Bin((*bin).to_string()), module_path(rest))
        }
        ["src", rest @ ..] => {
            let target = if pkg.dir.join("src/lib.rs").is_file() {
                Target::Lib
            } else {
                pkg.bin_for_path("src/main.rs")
                    .map_or(Target::Unknown, |b| Target::Bin(b.to_string()))
            };
            (target, module_path(rest))
        }
        [dir @ ("tests" | "benches" | "examples"), file] => {
            (aux_target(dir, stem(file)), String::new())
        }
        [dir @ ("tests" | "benches" | "examples"), sub, rest @ ..]
            if pkg.dir.join(dir).join(sub).join("main.rs").is_file() =>
        {
            (aux_target(dir, (*sub).to_string()), module_path(rest))
        }
        _ => (Target::Unknown, String::new()),
    }
}

/// The target of a file directly in `tests/`, `benches/` or `examples/`.
fn aux_target(dir: &str, name: String) -> Target {
    match dir {
        "tests" => Target::Test(name),
        "benches" => Target::Bench(name),
        _ => Target::Example(name),
    }
}

/// Module path of a file below its crate root's directory:
/// `["foo", "bar.rs"]` → `foo::bar`, `["foo", "mod.rs"]` → `foo`, and the
/// crate root itself (`["main.rs"]`, `["lib.rs"]`) → ``.
fn module_path(parts: &[&str]) -> String {
    if matches!(parts, ["main.rs" | "lib.rs"]) {
        return String::new();
    }
    let mut segments: Vec<String> = parts
        .iter()
        .map(|p| p.trim_end_matches(".rs").to_string())
        .collect();
    if segments.last().is_some_and(|l| l == "mod") {
        segments.pop();
    }
    segments.retain(|s| !s.is_empty());
    segments.join("::")
}
