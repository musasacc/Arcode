//! Finding a project's manifests (nested ones too) and reading each with
//! its manager and lockfile.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use super::node_locks::NodeLock;
use super::{Manager, ProjectManifest, scan_python};
use super::{bundler, cargo_deps, composer, go_deps, node_deps, node_locks};
use crate::fsutil;
use crate::node::{NodePackageManager, node_package_manager};

/// Folders never searched for manifests.
const SKIP_DIRS: [&str; 16] = [
    "node_modules",
    "target",
    "vendor",
    "venv",
    "env",
    "__pycache__",
    "dist",
    "build",
    "out",
    "site-packages",
    "bower_components",
    "Pods",
    "DerivedData",
    "coverage",
    "tmp",
    "third_party",
];
/// Deepest folder level searched below the root.
const MAX_DEPTH: usize = 6;
/// Most folders visited.
const MAX_DIRS: usize = 5_000;
/// Most manifests listed.
const MAX_MANIFESTS: usize = 200;
/// Larger files are not read.
const MAX_FILE: u64 = 32 * 1024 * 1024;

/// Lockfiles and other files whose change changes what the view shows.
const LOCK_FILES: [&str; 14] = [
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "bun.lock",
    "bun.lockb",
    "uv.lock",
    "poetry.lock",
    "Pipfile.lock",
    "composer.lock",
    "Gemfile.lock",
    ".yarnrc.yml",
];

fn is_manifest_name(name: &str, parent: Option<&str>) -> bool {
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "Pipfile"
            | "go.mod"
            | "composer.json"
            | "Gemfile"
    ) || is_requirements(name, parent)
}

fn is_requirements(name: &str, parent: Option<&str>) -> bool {
    name.ends_with(".txt") && (name.starts_with("requirements") || parent == Some("requirements"))
}

/// Whether a change to `path` can change the Packages view (a manifest or
/// a lockfile).
pub fn is_package_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    is_manifest_name(name, parent) || LOCK_FILES.contains(&name)
}

/// The manifests under `root` (the root's first, then by folder),
/// each read with its lockfile. Dependency folders (`node_modules`,
/// `target`, virtualenvs, hidden folders, ...) are not searched.
///
/// Blocks on the filesystem: call it off the UI thread.
pub fn scan_manifests(root: &Path) -> Vec<ProjectManifest> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut visited = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_DIRS || found.len() >= MAX_MANIFESTS {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let parent = dir.file_name().and_then(|n| n.to_str()).map(str::to_string);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let skip = name.starts_with('.') || SKIP_DIRS.contains(&name.as_str());
                if !skip && depth < MAX_DEPTH {
                    queue.push_back((entry.path(), depth + 1));
                }
            } else if kind.is_file() && is_manifest_name(&name, parent.as_deref()) {
                found.push(entry.path());
            }
        }
    }
    found.truncate(MAX_MANIFESTS);
    // By folder (a folder sorts before the ones inside it), then name.
    found.sort_by(|a, b| {
        a.parent()
            .cmp(&b.parent())
            .then(a.file_name().cmp(&b.file_name()))
    });
    found
        .iter()
        .filter_map(|p| read_manifest(p, root))
        .collect()
}

/// Reads one manifest (inside `root`, where lockfile searches stop);
/// `None` when it is not a manifest (or a `pyproject.toml` that only
/// configures tools).
pub fn read_manifest(path: &Path, root: &Path) -> Option<ProjectManifest> {
    let name = path.file_name()?.to_str()?;
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    let m = match name {
        "Cargo.toml" => read_cargo(path, root),
        "package.json" => read_node(path, root),
        "pyproject.toml" => return scan_python::read_pyproject(path, root),
        "Pipfile" => scan_python::read_pipfile(path, root),
        "go.mod" => read_simple(path, Manager::Go, |m, text| {
            let module = go_deps::parse(text);
            m.name = module.module;
            m.deps = module.deps;
            Ok(())
        }),
        "composer.json" => read_composer(path),
        "Gemfile" => read_bundler(path),
        n if is_requirements(n, parent) => scan_python::read_requirements(path, root),
        _ => return None,
    };
    Some(m)
}

pub(super) fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE {
        return Err(format!("{} is too large to read", file_name(path)));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Notes a lockfile problem without hiding the manifest's dependencies.
pub(super) fn lock_error(m: &mut ProjectManifest, lock: &Path, err: String) {
    m.error
        .get_or_insert(format!("Cannot read {}: {err}", file_name(lock)));
}

/// Reads the manifest text and hands it to `parse`; errors land in
/// [`ProjectManifest::error`].
pub(super) fn read_simple(
    path: &Path,
    manager: Manager,
    parse: impl FnOnce(&mut ProjectManifest, &str) -> Result<(), String>,
) -> ProjectManifest {
    let mut m = ProjectManifest::new(manager, path.to_path_buf());
    match read_text(path).and_then(|text| parse(&mut m, &text)) {
        Ok(()) => {}
        Err(e) => m.error = Some(e),
    }
    m
}

/// The first of `start` and its ancestors (up to `root`) that has `file`.
pub(super) fn nearest(start: &Path, root: &Path, file: &str) -> Option<PathBuf> {
    fsutil::ancestors(start, Some(root))
        .into_iter()
        .map(|d| d.join(file))
        .find(|p| p.is_file())
}

fn read_cargo(path: &Path, root: &Path) -> ProjectManifest {
    let dir = path.parent().unwrap_or(root).to_path_buf();
    let inherited = fsutil::ancestors(&dir, Some(root))
        .into_iter()
        .filter_map(|d| std::fs::read_to_string(d.join("Cargo.toml")).ok())
        .map(|t| cargo_deps::workspace_requirements(&t))
        .find(|w| !w.is_empty())
        .unwrap_or_default();
    let mut m = read_simple(path, Manager::Cargo, |m, text| {
        let c = cargo_deps::parse(text, &inherited)?;
        m.name = c.name;
        m.workspace_root = c.is_workspace;
        m.deps = c.deps;
        Ok(())
    });
    if let Some(lock) = nearest(&dir, root, "Cargo.lock") {
        match read_text(&lock).and_then(|t| cargo_deps::lock_versions(&t)) {
            Ok(versions) => {
                for dep in &mut m.deps {
                    if let Some(list) = versions.get(&dep.package) {
                        dep.locked = cargo_deps::pick_version(&dep.requested, list);
                    }
                }
            }
            Err(e) => lock_error(&mut m, &lock, e),
        }
        m.lockfile = Some(lock);
    }
    m
}

fn read_node(path: &Path, root: &Path) -> ProjectManifest {
    let mut workspaces = Vec::new();
    let mut m = read_simple(path, Manager::Npm, |m, text| {
        let n = node_deps::parse(text)?;
        m.name = n.name;
        m.deps = n.deps;
        workspaces = n.workspaces;
        Ok(())
    });
    let (pm, decided) = node_package_manager(&m.dir, Some(root));
    m.manager = match pm {
        NodePackageManager::Npm => Manager::Npm,
        NodePackageManager::Pnpm => Manager::Pnpm,
        NodePackageManager::Yarn => Manager::Yarn,
        NodePackageManager::Bun => Manager::Bun,
    };
    m.workspace_root = !workspaces.is_empty()
        || (pm == NodePackageManager::Pnpm && m.dir.join("pnpm-workspace.yaml").is_file());
    let locks: &[&str] = match pm {
        NodePackageManager::Npm => &["package-lock.json", "npm-shrinkwrap.json"],
        NodePackageManager::Pnpm => &["pnpm-lock.yaml"],
        NodePackageManager::Yarn => &["yarn.lock"],
        NodePackageManager::Bun => &["bun.lock", "bun.lockb"],
    };
    let Some(lock) = locks.iter().map(|l| decided.join(l)).find(|p| p.is_file()) else {
        return m;
    };
    m.lockfile = Some(lock.clone());
    if pm == NodePackageManager::Yarn {
        let berry_lock = std::fs::read_to_string(&lock).is_ok_and(|t| t.contains("__metadata:"));
        m.yarn_berry = berry_lock || decided.join(".yarnrc.yml").is_file();
    }
    if lock.extension().is_some_and(|e| e == "lockb") {
        return m;
    }
    let member = match fsutil::rel_slash(&m.dir, &decided) {
        rel if rel == "." => String::new(),
        rel => rel,
    };
    let parsed: Result<NodeLock, String> =
        read_text(&lock).and_then(|t| node_locks::parse(&file_name(&lock), &t));
    match parsed {
        Ok(versions) => {
            for dep in &mut m.deps {
                dep.locked = versions.version(&member, &dep.name, &dep.requested);
            }
        }
        Err(e) => lock_error(&mut m, &lock, e),
    }
    m
}

fn read_composer(path: &Path) -> ProjectManifest {
    let mut m = read_simple(path, Manager::Composer, |m, text| {
        let c = composer::parse(text)?;
        m.name = c.name;
        m.deps = c.deps;
        Ok(())
    });
    let lock = m.dir.join("composer.lock");
    if lock.is_file() {
        match read_text(&lock).and_then(|t| composer::lock_versions(&t)) {
            Ok(v) => set_locked(&mut m, &v, |n| n.to_ascii_lowercase()),
            Err(e) => lock_error(&mut m, &lock, e),
        }
        m.lockfile = Some(lock);
    }
    m
}

fn read_bundler(path: &Path) -> ProjectManifest {
    let mut m = read_simple(path, Manager::Bundler, |m, text| {
        m.deps = bundler::parse(text);
        Ok(())
    });
    let lock = m.dir.join("Gemfile.lock");
    if lock.is_file() {
        match read_text(&lock) {
            Ok(t) => set_locked(&mut m, &bundler::lock_versions(&t), str::to_string),
            Err(e) => lock_error(&mut m, &lock, e),
        }
        m.lockfile = Some(lock);
    }
    m
}

fn set_locked(
    m: &mut ProjectManifest,
    versions: &HashMap<String, String>,
    key: impl Fn(&str) -> String,
) {
    for dep in &mut m.deps {
        dep.locked = versions.get(&key(&dep.name)).cloned();
    }
}
