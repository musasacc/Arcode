//! Which manifests of a project have dependencies that are not installed,
//! for the "This project has packages that are not installed" banner.
//!
//! Only cheap, offline checks: file reads and `stat` calls. A Cargo project
//! without `target/` is never reported (Cargo fetches on the first build);
//! a project whose state cannot be told is not reported either.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::command::args;
use crate::package_files::{
    InstallCommand, MANIFESTS, PackageFile, PackageKind, PythonManifest, detect_within, make,
    package_file_for_manifest,
};
use crate::{fsutil, gomod, install_state, toolchain};

/// A manifest whose dependencies are missing, and how to install them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPackages {
    /// The manifest.
    pub file: PackageFile,
    /// Commands to run one after the other in a terminal: usually one, two
    /// for a Python project without a virtualenv (create `.venv`, then
    /// install into it).
    pub steps: Vec<InstallCommand>,
}

impl MissingPackages {
    /// The steps as one line for display, e.g. `npm install`.
    pub fn display(&self) -> String {
        self.steps
            .iter()
            .map(|s| s.display.as_str())
            .collect::<Vec<_>>()
            .join(" && ")
    }
}

/// The manifests in `root` and the nearest manifest of each of `opened`
/// (inside `root`) whose dependencies are not installed. Python manifests
/// of one folder and Node packages of one workspace are reported once.
///
/// Blocks on the filesystem: call it off the UI thread.
pub fn missing_packages(root: &Path, opened: &[PathBuf]) -> Vec<MissingPackages> {
    missing_packages_with(root, opened, gomod::module_cache().as_deref())
}

/// [`missing_packages`] with an explicit Go module cache (tests).
pub fn missing_packages_with(
    root: &Path,
    opened: &[PathBuf],
    go_cache: Option<&Path>,
) -> Vec<MissingPackages> {
    let mut files: Vec<PackageFile> = MANIFESTS
        .iter()
        .map(|name| root.join(name))
        .filter(|p| p.is_file())
        .filter_map(|p| package_file_for_manifest(&p, Some(root)))
        .collect();
    for path in opened.iter().filter(|p| p.starts_with(root)) {
        if let Some(file) = detect_within(path, Some(root)) {
            files.push(file);
        }
    }
    let mut out: Vec<MissingPackages> = Vec::new();
    for file in files {
        let duplicate = out.iter().any(|m| same_install(&m.file, &file));
        if duplicate || !is_missing(&file, go_cache) {
            continue;
        }
        let steps = install_steps(&file);
        out.push(MissingPackages { file, steps });
    }
    out
}

/// Two manifests installed by the same command (Python files of one
/// folder, packages of one Node workspace).
fn same_install(a: &PackageFile, b: &PackageFile) -> bool {
    match (a.kind, b.kind) {
        (PackageKind::Python(_), PackageKind::Python(_)) => {
            a.manifest.parent() == b.manifest.parent()
        }
        _ => a.manifest == b.manifest || (a.kind == b.kind && a.install == b.install),
    }
}

/// Whether `file` declares dependencies that are not installed.
pub fn is_missing(file: &PackageFile, go_cache: Option<&Path>) -> bool {
    if file.kind == PackageKind::Cargo || !has_dependencies(file) {
        return false;
    }
    match file.kind {
        PackageKind::Go => {
            let (Some(dir), Some(cache)) = (file.manifest.parent(), go_cache) else {
                return false;
            };
            let text = std::fs::read_to_string(&file.manifest).unwrap_or_default();
            gomod::modules_missing(dir, &text, cache)
        }
        _ => install_state(file) == crate::PackagesState::NotInstalled,
    }
}

/// Whether the manifest lists any dependency at all (an empty
/// `package.json` or `requirements.txt` needs no install).
pub fn has_dependencies(file: &PackageFile) -> bool {
    let path = &file.manifest;
    match file.kind {
        PackageKind::Cargo => true,
        PackageKind::Npm => fsutil::read_json(path).is_some_and(|json| {
            ["dependencies", "devDependencies", "optionalDependencies"]
                .iter()
                .any(|s| non_empty_object(json.get(s)))
        }),
        PackageKind::Composer => fsutil::read_json(path).is_some_and(|json| {
            ["require", "require-dev"].iter().any(|s| {
                json.get(s)
                    .and_then(Value::as_object)
                    .is_some_and(|m| m.keys().any(|k| k != "php" && !k.starts_with("ext-")))
            })
        }),
        PackageKind::Python(PythonManifest::Requirements) => lines(path).any(|l| !l.is_empty()),
        PackageKind::Python(PythonManifest::Pipfile) => fsutil::read_toml(path).is_some_and(|t| {
            ["packages", "dev-packages"].iter().any(|s| {
                t.get(*s)
                    .and_then(|v| v.as_table())
                    .is_some_and(|m| !m.is_empty())
            })
        }),
        PackageKind::Python(PythonManifest::Pyproject) => pyproject_has_dependencies(path),
        PackageKind::Go => {
            std::fs::read_to_string(path).is_ok_and(|text| !gomod::requirements(&text).is_empty())
        }
        PackageKind::Bundler => {
            lines(path).any(|l| l.starts_with("gem ") || l.starts_with("gemspec"))
        }
    }
}

fn non_empty_object(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_object)
        .is_some_and(|m| !m.is_empty())
}

/// Lines without comments (`#`) and surrounding blanks.
fn lines(path: &Path) -> impl Iterator<Item = String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

fn pyproject_has_dependencies(path: &Path) -> bool {
    let Some(table) = fsutil::read_toml(path) else {
        return false;
    };
    let project = table.get("project");
    let listed = |v: Option<&toml::Value>| {
        v.is_some_and(|v| match v {
            toml::Value::Array(a) => !a.is_empty(),
            toml::Value::Table(t) => !t.is_empty(),
            _ => false,
        })
    };
    let poetry = table
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
        .is_some_and(|d| d.keys().any(|k| k != "python"));
    listed(project.and_then(|p| p.get("dependencies")))
        || listed(project.and_then(|p| p.get("optional-dependencies")))
        || listed(table.get("dependency-groups"))
        || poetry
}

/// The commands that install `file`'s dependencies. A pip install that
/// would go to the system interpreter (no project virtualenv) creates
/// `.venv` first and installs into it, since that is what the banner
/// checks for and what current Linux distributions require.
pub fn install_steps(file: &PackageFile) -> Vec<InstallCommand> {
    let install = &file.install;
    let system_pip = install.program == toolchain::default_python()
        && install.args.starts_with(&args(&["-m", "pip"]));
    if !system_pip {
        return vec![install.clone()];
    }
    let cwd = &install.cwd;
    let create = make(
        toolchain::default_python(),
        args(&["-m", "venv", ".venv"]),
        cwd,
        None,
    );
    let python = toolchain::venv_python(&cwd.join(".venv"));
    let shown = fsutil::rel_slash(&python, cwd);
    let pip = make(
        &python.to_string_lossy(),
        install.args.clone(),
        cwd,
        Some(&shown),
    );
    vec![create, pip]
}

#[cfg(test)]
#[path = "missing_tests.rs"]
mod tests;
