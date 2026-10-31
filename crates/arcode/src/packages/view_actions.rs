//! Per-dependency commands of the Packages view (update, remove with
//! confirmation, documentation) and its context menu.

use std::path::Path;

use arcode_project::deps::{DepKind, Dependency, PackageAction};
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
            DepCommand::Remove => {
                self.confirm_remove = Some((path, d.name));
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
        let Some((path, name)) = self.confirm_remove.take() else {
            return;
        };
        let dep: Option<Dependency> = self
            .store
            .read(cx)
            .manifest(&path)
            .and_then(|m| m.deps.iter().find(|d| d.name == name).cloned());
        if let Some(d) = dep {
            let action = PackageAction::Remove {
                name: d.name,
                kind: d.kind,
                group: d
                    .group
                    .filter(|_| matches!(d.kind, DepKind::Dev | DepKind::Optional)),
            };
            self.run(&path, action, cx);
        }
        cx.notify();
    }

    /// Opens the context menu of a dependency row at `position`.
    pub fn open_menu(
        &mut self,
        section: usize,
        dep: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(DepMenu {
            section,
            dep,
            position,
        });
        cx.notify();
    }

    /// The newest version a check found for `dep` of the manifest at `path`.
    pub(super) fn newer(&self, path: &Path, dep: &Dependency, cx: &App) -> Option<String> {
        match self.store.read(cx).check(path) {
            Some(CheckState::Done(found)) => {
                let latest = found.get(&dep.package).or_else(|| found.get(&dep.name))?;
                super::rows::is_newer(dep, latest).then(|| latest.clone())
            }
            _ => None,
        }
    }
}
