//! Package manifests (`Cargo.toml`, `package.json`, ...) and the command
//! that installs their dependencies.
//!
//! Nothing here runs the install: the UI shows [`InstallCommand::display`]
//! in a banner and runs it in the integrated terminal on request.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::{args, command_line};
use crate::{fsutil, node, toolchain};

/// The kind of Python manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PythonManifest {
    /// `requirements.txt` (or `requirements-*.txt` when opened directly).
    Requirements,
    /// `pyproject.toml`.
    Pyproject,
    /// `Pipfile`.
    Pipfile,
}

/// The ecosystem of a package manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageKind {
    /// `Cargo.toml`; installs with `cargo build` (fetches and builds, so the
    /// `target/` directory [`crate::install_state`] looks for exists).
    Cargo,
    /// `package.json` (npm, pnpm, yarn or bun).
    Npm,
    /// A Python manifest.
    Python(PythonManifest),
    /// `go.mod`.
    Go,
    /// Ruby `Gemfile`.
    Bundler,
    /// PHP `composer.json`.
    Composer,
}

impl PackageKind {
    /// The kind of manifest a file name denotes, if any.
    pub fn from_file_name(name: &str) -> Option<Self> {
        Some(match name {
            "Cargo.toml" => Self::Cargo,
            "package.json" => Self::Npm,
            "pyproject.toml" => Self::Python(PythonManifest::Pyproject),
            "Pipfile" => Self::Python(PythonManifest::Pipfile),
            "go.mod" => Self::Go,
            "Gemfile" => Self::Bundler,
            "composer.json" => Self::Composer,
            n if n.starts_with("requirements") && n.ends_with(".txt") => {
                Self::Python(PythonManifest::Requirements)
            }
            _ => return None,
        })
    }

    /// A short human-readable name, e.g. `"Cargo"` or `"Python"`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo",
            Self::Npm => "Node",
            Self::Python(_) => "Python",
            Self::Go => "Go",
            Self::Bundler => "Bundler",
            Self::Composer => "Composer",
        }
    }
}

/// Manifest file names searched when walking up, in precedence order.
const MANIFESTS: [&str; 8] = [
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "Pipfile",
    "requirements.txt",
    "go.mod",
    "Gemfile",
    "composer.json",
];

/// A command that installs a project's dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallCommand {
    /// Program name or absolute path (e.g. a virtualenv interpreter).
    pub program: String,
    /// Arguments, unquoted.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// The command line to show, with paths relative to `cwd`.
    pub display: String,
}

/// A detected package manifest and how to install its dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFile {
    /// Ecosystem.
    pub kind: PackageKind,
    /// Absolute path of the manifest.
    pub manifest: PathBuf,
    /// The outermost directory considered (boundary or version-control root).
    pub project_root: PathBuf,
    /// The install command.
    pub install: InstallCommand,
}

/// Finds the package manifest for an opened file: the file itself if it is a
/// manifest, else the nearest one in its directory or above (up to the
/// version-control root). Manifests matching the file's language win, so a
/// `.py` file in a repo with both `package.json` and `pyproject.toml` maps
/// to the latter.
pub fn detect(path: &Path) -> Option<PackageFile> {
    detect_within(path, None)
}

/// Like [`detect`], but never looks above `boundary` (the opened folder).
pub fn detect_within(path: &Path, boundary: Option<&Path>) -> Option<PackageFile> {
    if path.is_file()
        && let Some(file) = package_file_for_manifest(path, boundary)
    {
        return Some(file);
    }
    let dirs = fsutil::ancestors(&fsutil::start_dir(path), boundary);
    let preferred = preferred_manifests(path);
    let find = |names: &[&str]| {
        dirs.iter()
            .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
    };
    let manifest = find(&preferred).or_else(|| find(&MANIFESTS))?;
    package_file_for_manifest(&manifest, boundary)
}

/// Builds the [`PackageFile`] for a manifest path; `None` if the file name is
/// not a known manifest.
pub fn package_file_for_manifest(manifest: &Path, boundary: Option<&Path>) -> Option<PackageFile> {
    let kind = PackageKind::from_file_name(manifest.file_name()?.to_str()?)?;
    let dir = manifest.parent()?.to_path_buf();
    let project_root = fsutil::walk_root(&dir, boundary);
    let install = install_command(kind, manifest, &dir, boundary);
    Some(PackageFile {
        kind,
        manifest: manifest.to_path_buf(),
        project_root,
        install,
    })
}

/// Manifest names matching the opened file's extension.
fn preferred_manifests(path: &Path) -> Vec<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs" => vec!["Cargo.toml"],
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "vue" | "svelte" => {
            vec!["package.json"]
        }
        "py" | "pyi" => vec!["pyproject.toml", "Pipfile", "requirements.txt"],
        "go" => vec!["go.mod"],
        "rb" => vec!["Gemfile"],
        "php" => vec!["composer.json"],
        _ => Vec::new(),
    }
}

/// The install command for a manifest in `dir`.
fn install_command(
    kind: PackageKind,
    manifest: &Path,
    dir: &Path,
    boundary: Option<&Path>,
) -> InstallCommand {
    let plain = |program: &str, list: &[&str]| make(program, args(list), dir, None);
    match kind {
        PackageKind::Cargo => plain("cargo", &["build"]),
        PackageKind::Go => plain("go", &["mod", "download"]),
        PackageKind::Bundler => plain("bundle", &["install"]),
        PackageKind::Composer => plain("composer", &["install"]),
        PackageKind::Npm => {
            let (pm, root) = node::node_package_manager(dir, boundary);
            make(pm.program(), args(&["install"]), &root, None)
        }
        PackageKind::Python(PythonManifest::Pipfile) => plain("pipenv", &["install"]),
        PackageKind::Python(PythonManifest::Pyproject) => pyproject_install(dir, boundary),
        PackageKind::Python(PythonManifest::Requirements) => {
            let file = manifest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            pip_install(dir, boundary, vec!["-r".to_string(), file])
        }
    }
}

/// `uv sync` / `poetry install` / `pdm install` by lockfile, else
/// `pip install -e .`.
fn pyproject_install(dir: &Path, boundary: Option<&Path>) -> InstallCommand {
    let locks = [
        ("uv.lock", "uv", "sync"),
        ("poetry.lock", "poetry", "install"),
        ("pdm.lock", "pdm", "install"),
    ];
    for d in fsutil::ancestors(dir, boundary) {
        if let Some((_, program, sub)) = locks.iter().find(|(f, _, _)| d.join(f).is_file()) {
            return make(program, args(&[sub]), &d, None);
        }
    }
    pip_install(dir, boundary, args(&["-e", "."]))
}

/// `<python> -m pip install <rest>`, preferring the project's virtualenv.
fn pip_install(dir: &Path, boundary: Option<&Path>, rest: Vec<String>) -> InstallCommand {
    let mut list = args(&["-m", "pip", "install"]);
    list.extend(rest);
    match toolchain::find_python_venv(dir, boundary) {
        Some(venv) => {
            let shown = fsutil::rel_slash(&venv.python, dir);
            make(&venv.python.to_string_lossy(), list, dir, Some(&shown))
        }
        None => make(toolchain::default_python(), list, dir, None),
    }
}

fn make(
    program: &str,
    args: Vec<String>,
    cwd: &Path,
    shown_program: Option<&str>,
) -> InstallCommand {
    let display = command_line(shown_program.unwrap_or(program), &args);
    InstallCommand {
        program: program.to_string(),
        args,
        cwd: cwd.to_path_buf(),
        display,
    }
}
