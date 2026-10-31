//! Package managers of a project and the dependencies they declare, for
//! the Packages view.
//!
//! Everything here is offline: [`scan_manifests`] finds the manifests of a
//! project (nested ones too) and reads each with its lockfile into a
//! [`ProjectManifest`]. [`action_command`] and [`outdated_command`] build
//! the manager's own commands (as separate arguments, never a shell line);
//! the UI runs them on request. [`parse_outdated`] reads the machine
//! readable output of a check for newer versions, and [`capture`] runs
//! such a check in the background with a way to cancel it.
//!
//! Parsers never panic: a malformed file gives a manifest with an
//! [`ProjectManifest::error`] and whatever could be read.

mod bundler;
mod capture;
mod cargo_deps;
mod commands;
mod composer;
mod go_deps;
mod lines;
mod mini_yaml;
mod node_deps;
mod node_locks;
mod outdated;
mod python_deps;
mod python_locks;
mod requirements;
mod scan;
mod scan_python;
mod tools;
mod validate;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod review_tests;

use std::path::PathBuf;

pub use capture::{CancelFlag, CapturedOutput, capture};
pub use commands::{PackageAction, PackageCommand, action_command};
pub use outdated::{OutdatedFormat, OutdatedPackage, outdated_command, parse_outdated};
pub use scan::{is_package_file, read_manifest, scan_manifests};
pub use tools::{find_program, install_hint};
pub use validate::{validate_package_name, validate_version};

/// A package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Manager {
    /// Rust's Cargo.
    Cargo,
    /// npm.
    Npm,
    /// pnpm.
    Pnpm,
    /// Yarn (classic or berry).
    Yarn,
    /// Bun.
    Bun,
    /// uv (Python).
    Uv,
    /// Poetry (Python).
    Poetry,
    /// Pipenv (Python).
    Pipenv,
    /// pip with `requirements.txt` or a plain `pyproject.toml`.
    Pip,
    /// Go modules.
    Go,
    /// PHP Composer.
    Composer,
    /// Ruby Bundler.
    Bundler,
}

impl Manager {
    /// The name shown on the manager badge.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
            Self::Uv => "uv",
            Self::Poetry => "poetry",
            Self::Pipenv => "pipenv",
            Self::Pip => "pip",
            Self::Go => "go",
            Self::Composer => "composer",
            Self::Bundler => "bundler",
        }
    }

    /// The executable that runs the manager's commands (pip runs through
    /// a Python interpreter instead, see [`ProjectManifest::python`]).
    pub fn program(self) -> &'static str {
        match self {
            Self::Bundler => "bundle",
            Self::Pip => "python",
            other => other.label(),
        }
    }

    /// Whether `add` can put the package among development dependencies.
    pub fn supports_dev(self) -> bool {
        !matches!(self, Self::Go | Self::Pip)
    }

    /// The registry page of `package`, if it is a valid package name.
    pub fn docs_url(self, package: &str) -> Option<String> {
        validate_package_name(package).ok()?;
        Some(match self {
            Self::Cargo => format!("https://docs.rs/{package}"),
            Self::Npm | Self::Pnpm | Self::Yarn | Self::Bun => {
                format!("https://www.npmjs.com/package/{package}")
            }
            Self::Uv | Self::Poetry | Self::Pipenv | Self::Pip => {
                format!("https://pypi.org/project/{package}/")
            }
            Self::Go => format!("https://pkg.go.dev/{package}"),
            Self::Composer => format!("https://packagist.org/packages/{package}"),
            Self::Bundler => format!("https://rubygems.org/gems/{package}"),
        })
    }

    /// Whether this is a Node package manager.
    pub fn is_node(self) -> bool {
        matches!(self, Self::Npm | Self::Pnpm | Self::Yarn | Self::Bun)
    }

    /// Whether this is a Python package manager.
    pub fn is_python(self) -> bool {
        matches!(self, Self::Uv | Self::Poetry | Self::Pipenv | Self::Pip)
    }
}

/// How a dependency is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepKind {
    /// A regular (runtime) dependency.
    Normal,
    /// Only for development and tests.
    Dev,
    /// Only for build scripts (Cargo).
    Build,
    /// Optional (a Cargo feature, an npm optional dependency, a Python
    /// extra).
    Optional,
    /// A peer dependency (npm).
    Peer,
    /// Required indirectly (`// indirect` in `go.mod`).
    Indirect,
}

impl DepKind {
    /// Short label for the row badge (`""` for normal dependencies).
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "",
            Self::Dev => "dev",
            Self::Build => "build",
            Self::Optional => "optional",
            Self::Peer => "peer",
            Self::Indirect => "indirect",
        }
    }
}

/// Where a dependency comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DepSource {
    /// The manager's registry (crates.io, npm, PyPI, ...).
    Registry,
    /// A git repository (its URL).
    Git(String),
    /// A local folder (as written).
    Path(String),
    /// Another member of the same workspace (or inherited from it).
    Workspace,
    /// A direct URL (tarball, wheel).
    Url(String),
}

impl DepSource {
    /// Short label for the row (`""` for the registry).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Registry => "",
            Self::Git(_) => "git",
            Self::Path(_) => "path",
            Self::Workspace => "workspace",
            Self::Url(_) => "url",
        }
    }
}

/// One declared dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The name as written in the manifest (the key, which differs from
    /// [`Dependency::package`] for renamed Cargo dependencies and npm
    /// aliases).
    pub name: String,
    /// The package's name in its registry.
    pub package: String,
    /// The requested version or range as written (`""` when none).
    pub requested: String,
    /// The version the lockfile (or the environment) has.
    pub locked: Option<String>,
    /// Normal, dev, build, ...
    pub kind: DepKind,
    /// A dependency group, extra, Cargo target or Bundler group.
    pub group: Option<String>,
    /// Registry, git, path, ...
    pub source: DepSource,
    /// Zero-based line of the declaration in the manifest.
    pub line: Option<usize>,
}

impl Dependency {
    /// A registry dependency of `kind` with `requested` and nothing else.
    pub fn new(name: &str, requested: &str, kind: DepKind) -> Self {
        Self {
            name: name.to_string(),
            package: name.to_string(),
            requested: requested.to_string(),
            locked: None,
            kind,
            group: None,
            source: DepSource::Registry,
            line: None,
        }
    }
}

/// A manifest with its manager and dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectManifest {
    /// The package manager.
    pub manager: Manager,
    /// Absolute path of the manifest.
    pub path: PathBuf,
    /// Its folder, where the manager's commands run.
    pub dir: PathBuf,
    /// The package or project name the manifest declares.
    pub name: Option<String>,
    /// The lockfile versions were read from.
    pub lockfile: Option<PathBuf>,
    /// The Python interpreter pip runs with (the project's virtualenv, if
    /// any; else the system's).
    pub python: Option<PathBuf>,
    /// Yarn 2+ (`yarn up` instead of `yarn upgrade`, no `outdated`).
    pub yarn_berry: bool,
    /// Whether this is a workspace root (Cargo, npm/pnpm/yarn).
    pub workspace_root: bool,
    /// Declared dependencies in manifest order.
    pub deps: Vec<Dependency>,
    /// Why the manifest (or its lockfile) could not be read completely.
    pub error: Option<String>,
}

impl ProjectManifest {
    /// An empty manifest of `manager` at `path`.
    pub fn new(manager: Manager, path: PathBuf) -> Self {
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        Self {
            manager,
            path,
            dir,
            name: None,
            lockfile: None,
            python: None,
            yarn_berry: false,
            workspace_root: false,
            deps: Vec::new(),
            error: None,
        }
    }

    /// The manifest's file name (`Cargo.toml`, `requirements-dev.txt`).
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// PEP 503 normalized Python name: lower case, runs of `-_.` become `-`.
pub(crate) fn normalize_python(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            sep = true;
            continue;
        }
        if sep && !out.is_empty() {
            out.push('-');
        }
        sep = false;
        out.push(c.to_ascii_lowercase());
    }
    out
}
