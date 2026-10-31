//! Per-dependency commands of the Packages view (update, remove with
//! confirmation, documentation) and its context menu.

use std::path::{Path, PathBuf};

use arcode_project::deps::{
    DepKind, Dependency, Manager, PackageAction, ProjectManifest, action_command, normalize_python,
};
use gpui::{App, Context, Pixels, Point};

use super::forms::DepMenu;
use super::store::CheckState;
use super::view::{PackagesEvent, PackagesView};

/// A per-dependency command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepCommand {
    /// Open the manifest at the declaration.
    Open,
    /// Update the package.
    Update,
    /// Remove it (after confirmation).
    Remove,
    /// Open its registry page.
    Docs,
}

/// A dependency of a manifest, independent of its position in the list
/// (which a rescan can change). A name can be declared more than once
/// (`tokio` in `[dependencies]` and `[dev-dependencies]`), so the kind and
/// group are part of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepKey {
    /// The manifest.
    pub manifest: PathBuf,
    /// The name as written.
    pub name: String,
    /// Normal, dev, ...
    pub kind: DepKind,
    /// Group, extra or Cargo target.
    pub group: Option<String>,
}

impl DepKey {
    /// The key of `dep` declared in the manifest at `manifest`.
    pub fn of(manifest: &Path, dep: &Dependency) -> Self {
        Self {
            manifest: manifest.to_path_buf(),
            name: dep.name.clone(),
            kind: dep.kind,
            group: dep.group.clone(),
        }
    }

    /// Whether `dep` of the manifest at `manifest` is this dependency.
    pub fn matches(&self, manifest: &Path, dep: &Dependency) -> bool {
        self.manifest == manifest
            && self.name == dep.name
            && self.kind == dep.kind
            && self.group == dep.group
    }
}

impl PackagesView {
    /// Runs `kind` (update, remove, docs) for dependency `dep` of section
    /// `section`. Removing asks for confirmation in the row first.
    pub fn dep_action(
        &mut self,
        section: usize,
        dep: usize,
        kind: DepCommand,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        let Some(m) = self.manifest(section, cx) else {
            return;
        };
        let Some(d) = m.deps.get(dep).cloned() else {
            return;
        };
        let (path, manager) = (m.path.clone(), m.manager);
        match kind {
            DepCommand::Open => self.open_dep(section, dep, cx),
            DepCommand::Update => self.run(&path, PackageAction::Update { name: d.package }, cx),
            // The command's error says why (a toast), no confirmation.
            DepCommand::Remove if !removable(m, &d) => self.run(&path, remove_action(&d), cx),
            DepCommand::Remove => {
                self.confirm_remove = Some(DepKey::of(&path, &d));
                cx.notify();
            }
            DepCommand::Docs => {
                if let Some(url) = manager.docs_url(&d.package) {
                    cx.emit(PackagesEvent::OpenUrl(url));
                }
            }
        }
    }

    /// Removes the dependency waiting for confirmation.
    pub fn confirm_removal(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.confirm_remove.take() else {
            return;
        };
        let path = key.manifest.clone();
        let dep: Option<Dependency> = self
            .store
            .read(cx)
            .manifest(&path)
            .and_then(|m| m.deps.iter().find(|d| key.matches(&path, d)).cloned());
        if let Some(d) = dep {
            self.run(&path, remove_action(&d), cx);
        }
        cx.notify();
    }

    /// After a rescan: closes the context menu if its row now shows
    /// another dependency (its entries act by position), and drops a
    /// removal confirmation whose dependency is gone.
    pub(super) fn forget_stale_targets(&mut self, cx: &App) {
        let store = self.store.read(cx);
        if let Some(menu) = &self.menu {
            let same = store
                .manifests()
                .get(menu.section)
                .and_then(|m| m.deps.get(menu.dep).map(|d| menu.key.matches(&m.path, d)))
                .unwrap_or(false);
            if !same {
                self.menu = None;
            }
        }
        if let Some(key) = &self.confirm_remove {
            let present = store
                .manifest(&key.manifest)
                .is_some_and(|m| m.deps.iter().any(|d| key.matches(&m.path, d)));
            if !present {
                self.confirm_remove = None;
            }
        }
    }

    /// Opens the context menu of a dependency row at `position`.
    pub fn open_menu(
        &mut self,
        section: usize,
        dep: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self
            .manifest(section, cx)
            .and_then(|m| m.deps.get(dep).map(|d| DepKey::of(&m.path, d)))
        else {
            return;
        };
        self.menu = Some(DepMenu {
            section,
            dep,
            key,
            position,
        });
        cx.notify();
    }

    /// The newest version a check found for `dep` of the manifest at `path`.
    pub(super) fn newer(&self, path: &Path, dep: &Dependency, cx: &App) -> Option<String> {
        match self.store.read(cx).check(path) {
            Some(CheckState::Done(found)) => {
                let manager = self.store.read(cx).manifest(path)?.manager;
                let latest = found
                    .get(&dep.package)
                    .or_else(|| found.get(&dep.name))
                    .or_else(|| {
                        // `found` only holds outdated packages: few.
                        found
                            .iter()
                            .find(|(name, _)| same_package(manager, name, &dep.package))
                            .map(|(_, v)| v)
                    })?;
                super::rows::is_newer(dep, latest).then(|| latest.clone())
            }
            _ => None,
        }
    }
}

/// The command removing `dep`. The manager's command decides what the
/// group means (a Python group or extra, a Cargo target) and whether it is
/// needed.
pub(super) fn remove_action(dep: &Dependency) -> PackageAction {
    PackageAction::Remove {
        name: dep.name.clone(),
        kind: dep.kind,
        group: dep.group.clone(),
    }
}

/// Whether the manager can remove `dep` from `m` (not an entry of
/// `[workspace.dependencies]`, nor any in a Cargo virtual manifest).
pub(super) fn removable(m: &ProjectManifest, dep: &Dependency) -> bool {
    action_command(m, &remove_action(dep), cfg!(windows)).is_ok()
}

/// Whether the Add form applies to `m` (not a Cargo virtual manifest,
/// where cargo cannot tell which member to change).
pub(super) fn can_add(m: &ProjectManifest) -> bool {
    !(m.manager == Manager::Cargo && m.virtual_manifest)
}

/// Whether a check reported `reported` for the package `declared`: Python
/// names compare PEP 503-normalized (pip reports `PyYAML`, uv `pyyaml`, a
/// manifest may write `Flask_Login`), Composer's case-insensitively.
fn same_package(manager: Manager, reported: &str, declared: &str) -> bool {
    if manager.is_python() {
        normalize_python(reported) == normalize_python(declared)
    } else if manager == Manager::Composer {
        reported.eq_ignore_ascii_case(declared)
    } else {
        false
    }
}
