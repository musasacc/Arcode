//! Node package manager detection and `package.json` helpers.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fsutil;

/// A Node package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodePackageManager {
    /// npm (the default when nothing else is detected).
    Npm,
    /// pnpm.
    Pnpm,
    /// Yarn (classic or berry).
    Yarn,
    /// Bun.
    Bun,
}

impl NodePackageManager {
    /// The executable name.
    pub fn program(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    /// Program and leading arguments that run a locally installed package
    /// binary without downloading anything (`npx --no-install`, `pnpm exec`,
    /// `yarn`).
    pub fn exec_prefix(self) -> (&'static str, Vec<String>) {
        match self {
            Self::Npm | Self::Bun => ("npx", vec!["--no-install".to_string()]),
            Self::Pnpm => ("pnpm", vec!["exec".to_string()]),
            Self::Yarn => ("yarn", Vec::new()),
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "npm" => Some(Self::Npm),
            "pnpm" => Some(Self::Pnpm),
            "yarn" => Some(Self::Yarn),
            "bun" => Some(Self::Bun),
            _ => None,
        }
    }
}

/// Lockfiles (and workspace markers) per manager, in precedence order.
const MARKERS: [(&str, NodePackageManager); 7] = [
    ("pnpm-lock.yaml", NodePackageManager::Pnpm),
    ("pnpm-workspace.yaml", NodePackageManager::Pnpm),
    ("yarn.lock", NodePackageManager::Yarn),
    ("bun.lockb", NodePackageManager::Bun),
    ("bun.lock", NodePackageManager::Bun),
    ("package-lock.json", NodePackageManager::Npm),
    ("npm-shrinkwrap.json", NodePackageManager::Npm),
];

/// Detects the package manager for the package in `dir`.
///
/// Walks from `dir` up to `boundary` (or the version-control root). In each
/// directory the `packageManager` field of `package.json` wins, then the
/// lockfiles (pnpm, yarn, bun, npm). Returns the manager and the directory
/// where it was decided (the workspace root for pnpm/yarn workspaces), or
/// npm and `dir` when nothing is found.
pub fn node_package_manager(dir: &Path, boundary: Option<&Path>) -> (NodePackageManager, PathBuf) {
    for d in fsutil::ancestors(dir, boundary) {
        if let Some(pm) = package_manager_field(&d.join("package.json")) {
            return (pm, d);
        }
        if let Some((_, pm)) = MARKERS.iter().find(|(f, _)| d.join(f).is_file()) {
            return (*pm, d);
        }
    }
    (NodePackageManager::Npm, dir.to_path_buf())
}

/// `"packageManager": "pnpm@9.1.0"` → pnpm.
fn package_manager_field(package_json: &Path) -> Option<NodePackageManager> {
    let json = fsutil::read_json(package_json)?;
    let spec = json.get("packageManager")?.as_str()?;
    NodePackageManager::from_name(spec.split('@').next()?)
}

/// Script names from `package.json` (order unspecified).
pub(crate) fn scripts(json: &Value) -> Vec<String> {
    json.get("scripts")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Whether `package.json` lists `name` in any dependency section.
pub(crate) fn has_dependency(json: &Value, name: &str) -> bool {
    [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ]
    .iter()
    .any(|section| json.get(section).and_then(|s| s.get(name)).is_some())
}

/// The nearest directory with a `package.json`, from `start` up to `boundary`.
pub(crate) fn nearest_package_dir(start: &Path, boundary: Option<&Path>) -> Option<PathBuf> {
    fsutil::ancestors(start, boundary)
        .into_iter()
        .find(|d| d.join("package.json").is_file())
}
