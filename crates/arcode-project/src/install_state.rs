//! Heuristics for whether a project's dependencies look installed, so the
//! banner can say "Packages not installed" rather than "Reinstall".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::package_files::{PackageFile, PackageKind, PythonManifest};
use crate::{fsutil, toolchain};

/// Whether dependencies appear to be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackagesState {
    /// The usual install output exists (`node_modules`, `target`, `.venv`...).
    Installed,
    /// The usual install output is missing.
    NotInstalled,
    /// Cannot tell (e.g. Go's global module cache, Poetry's external venvs).
    Unknown,
}

/// Inspects the filesystem around a [`PackageFile`].
///
/// - Node: `node_modules` (or Yarn PnP's `.pnp.cjs`) in the package dir or
///   any directory up to the project root.
/// - Cargo: a `target` directory there.
/// - Python: a virtual environment (`.venv`, `venv`, `env`); without one,
///   Pipfile and Poetry projects are `Unknown` since their venvs usually
///   live outside the project.
/// - Go: `vendor/` means installed, otherwise unknown.
/// - Bundler: `vendor/bundle` or `.bundle/config` means installed; a missing
///   `Gemfile.lock` means not installed.
/// - Composer: `vendor/`.
pub fn install_state(file: &PackageFile) -> PackagesState {
    let Some(dir) = file.manifest.parent() else {
        return PackagesState::Unknown;
    };
    let dirs = fsutil::ancestors(dir, Some(&file.project_root));
    let any_dir = |names: &[&str]| {
        dirs.iter()
            .any(|d| names.iter().any(|n| d.join(n).exists()))
    };
    let found = |yes: bool| {
        if yes {
            PackagesState::Installed
        } else {
            PackagesState::NotInstalled
        }
    };
    match file.kind {
        PackageKind::Npm => found(any_dir(&["node_modules", ".pnp.cjs", ".pnp.js"])),
        PackageKind::Cargo => found(dirs.iter().any(|d| d.join("target").is_dir())),
        PackageKind::Composer => found(dir.join("vendor").is_dir()),
        PackageKind::Go if dir.join("vendor").is_dir() => PackagesState::Installed,
        PackageKind::Go => PackagesState::Unknown,
        PackageKind::Bundler => bundler_state(dir),
        PackageKind::Python(manifest) => python_state(dir, &file.project_root, manifest, &dirs),
    }
}

/// `true` when dependencies look missing ([`PackagesState::NotInstalled`]).
pub fn needs_install(file: &PackageFile) -> bool {
    install_state(file) == PackagesState::NotInstalled
}

fn bundler_state(dir: &Path) -> PackagesState {
    if dir.join("vendor").join("bundle").is_dir() || dir.join(".bundle").join("config").is_file() {
        PackagesState::Installed
    } else if !dir.join("Gemfile.lock").is_file() {
        PackagesState::NotInstalled
    } else {
        PackagesState::Unknown
    }
}

fn python_state(
    dir: &Path,
    root: &Path,
    manifest: PythonManifest,
    dirs: &[PathBuf],
) -> PackagesState {
    if toolchain::find_python_venv(dir, Some(root)).is_some() {
        return PackagesState::Installed;
    }
    let external_venv = match manifest {
        PythonManifest::Pipfile => true,
        PythonManifest::Pyproject => dirs.iter().any(|d| d.join("poetry.lock").is_file()),
        PythonManifest::Requirements => false,
    };
    if external_venv {
        PackagesState::Unknown
    } else {
        PackagesState::NotInstalled
    }
}
