//! Packages activity wiring: the store and sidebar view, opening a
//! dependency's declaration, running manager commands in terminal tabs
//! (argument lists, no shell) and refreshing when manifests or lockfiles
//! change or a command exits.

use std::path::{Path, PathBuf};

use arcode_buffer::Point;
use arcode_project::deps::{PackageAction, PackageCommand, action_command, find_program};
use gpui::{AppContext, Context, Entity, Focusable, InteractiveElement, Subscription, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::bottom_panel::BottomTab;
use crate::packages::{
    AddPackage, CheckPackageUpdates, PackagesEvent, PackagesRequest, PackagesStore,
    PackagesStoreEvent, PackagesView, RefreshPackages, ShowPackages, UpdateAllPackages,
    not_installed,
};

/// The Packages entities of a workspace.
pub struct PackagesParts {
    /// Manifests and check results.
    pub store: Entity<PackagesStore>,
    /// The sidebar view.
    pub view: Entity<PackagesView>,
    /// Tests: commands are recorded here instead of started.
    #[cfg(test)]
    pub(crate) recorded: Option<Vec<PackageCommand>>,
}

impl PackagesParts {
    /// Creates the store and view and the workspace's subscriptions.
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> (Self, Vec<Subscription>) {
        let store = cx.new(PackagesStore::new);
        let view = cx.new(|cx| PackagesView::new(store.clone(), window, cx));
        let subs =
            vec![
                cx.subscribe_in(&view, window, |ws, _, event: &PackagesEvent, window, cx| {
                    match event {
                        PackagesEvent::Open { path, line } => {
                            let at = Point::new(*line, 0);
                            ws.open_at(path, at..at, window, cx);
                        }
                        PackagesEvent::Run { manifest, action } => {
                            ws.run_package_action(manifest, action, window, cx)
                        }
                        PackagesEvent::OpenUrl(url) => cx.open_url(url),
                    }
                }),
                cx.subscribe_in(
                    &store,
                    window,
                    |ws, _, event: &PackagesStoreEvent, window, cx| match event {
                        PackagesStoreEvent::Message(text) => ws.show_toast(text.clone(), cx),
                        PackagesStoreEvent::RunInTerminal { manifest, command } => {
                            ws.run_package_command(manifest, command.clone(), window, cx)
                        }
                    },
                ),
            ];
        let parts = Self {
            store,
            view,
            #[cfg(test)]
            recorded: None,
        };
        (parts, subs)
    }
}

impl Workspace {
    /// The Packages store and view.
    pub fn packages(&self) -> &PackagesParts {
        &self.pkgs
    }

    /// Points the Packages view at the project root (read on first use).
    pub(super) fn set_packages_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.pkgs.store.update(cx, |s, cx| s.set_root(root, cx));
    }

    /// Files changed on disk: manifests and lockfiles refresh the view.
    pub(super) fn packages_files_changed(
        &mut self,
        paths: &[PathBuf],
        overflow: bool,
        cx: &mut Context<Self>,
    ) {
        self.pkgs
            .store
            .update(cx, |s, cx| s.paths_changed(paths, overflow, cx));
    }

    /// Builds and runs the command of `action` for the manifest at
    /// `manifest`; invalid input is reported, not run.
    pub(super) fn run_package_action(
        &mut self,
        manifest: &Path,
        action: &PackageAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.pkgs.store.read(cx).manifest(manifest).cloned() else {
            return;
        };
        match action_command(&m, action, cfg!(windows)) {
            Ok(command) => self.run_package_command(manifest, command, window, cx),
            Err(err) => self.show_toast(err, cx),
        }
    }

    /// Runs `command` in a new terminal tab (directly, not through a
    /// shell). The view is read again when it exits.
    pub(super) fn run_package_command(
        &mut self,
        manifest: &Path,
        command: PackageCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        if let Some(recorded) = &mut self.pkgs.recorded {
            recorded.push(command);
            return;
        }
        let Some(m) = self.pkgs.store.read(cx).manifest(manifest).cloned() else {
            return;
        };
        if find_program(&command.program).is_none() {
            self.show_toast(not_installed(&m, &command), cx);
            return;
        }
        let store = self.pkgs.store.downgrade();
        let on_exit: crate::terminal_panel::OnExit = Box::new(move |cx| {
            store.update(cx, |s, cx| s.refresh(cx)).ok();
        });
        let terminals = self.bottom_panel.read(cx).terminals().clone();
        let started = terminals.update(cx, |t, cx| {
            t.run_program(&command, Some(on_exit), window, cx)
        });
        match started {
            Ok(()) => {
                self.bottom_panel
                    .update(cx, |p, cx| p.set_active_tab(BottomTab::Terminal, cx));
                self.bottom_panel_visible = true;
            }
            Err(err) => self.show_toast(format!("Cannot run {}: {err}", command.display), cx),
        }
        cx.notify();
    }

    /// Shows the Packages view.
    pub(super) fn show_packages(&mut self, cx: &mut Context<Self>) {
        self.activity = Activity::Packages;
        self.sidebar_visible = true;
        self.pkgs.store.update(cx, |s, cx| s.ensure_loaded(cx));
        cx.notify();
    }

    /// Shows the view and runs a palette request (for the active file's
    /// manifest).
    fn packages_request(
        &mut self,
        request: PackagesRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_root().is_none() {
            self.show_toast("Open a folder first", cx);
            return;
        }
        self.show_packages(cx);
        let hint = self
            .active_editor(cx)
            .and_then(|e| e.read(cx).path().map(Path::to_path_buf));
        self.pkgs
            .view
            .update(cx, |v, cx| v.request(request, hint, window, cx));
    }

    pub(super) fn register_packages_actions<E: InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(|ws, _: &ShowPackages, window, cx| {
            ws.show_packages(cx);
            window.focus(&ws.pkgs.view.focus_handle(cx));
        }))
        .on_action(cx.listener(|ws, _: &AddPackage, window, cx| {
            ws.packages_request(PackagesRequest::Add, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &UpdateAllPackages, window, cx| {
            ws.packages_request(PackagesRequest::UpdateAll, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &CheckPackageUpdates, window, cx| {
            ws.packages_request(PackagesRequest::CheckUpdates, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &RefreshPackages, _, cx| {
            ws.show_packages(cx);
            ws.pkgs.store.update(cx, |s, cx| s.refresh(cx));
        }))
    }
}
