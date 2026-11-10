//! Installing missing language servers from the workspace: the Install
//! button of the "not installed" message and of the server's status menu,
//! and "LSP: Show Missing Language Servers". See `lsp_bridge/install.rs`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use arcode_project::deps::PackageCommand;
use arcode_project::packs::find_executable;
use arcode_project::server_install::{InstallCommandLine, InstallHint};
use arcode_ui_kit::{IconName, settings};
use gpui::{AppContext as _, Context, Window};

use super::Workspace;
use super::overlays::ToastLevel;
use crate::bottom_panel::BottomTab;
use crate::languages::PickPurpose;
use crate::languages::resolve::language_for;
use crate::list_picker::PickItem;
use crate::lsp_bridge::install::{
    InstallLanguageServer, NotInstalled, hint_for, missing_servers, server_command, servers_of,
};

impl Workspace {
    /// Shows a "not installed" message with its button.
    pub(super) fn show_not_installed(&mut self, report: &NotInstalled, cx: &mut Context<Self>) {
        let action = InstallLanguageServer {
            server: report.server.clone(),
        };
        self.show_toast_with_action(
            report.message.clone(),
            ToastLevel::Warning,
            report.label.clone(),
            Box::new(action),
            cx,
        );
    }

    /// Installs `server`: runs its install command in a new terminal tab,
    /// or opens the page that says how. Nothing runs without this call
    /// (a click on Install).
    pub fn install_language_server(
        &mut self,
        server: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hint) = self.lsp.read(cx).install_hint(server).cloned() {
            self.run_install_hint(hint, window, cx);
            return;
        }
        let command = server_command(server, settings(cx));
        let server = server.to_string();
        let work = cx.background_spawn(async move { hint_for(&command) });
        cx.spawn_in(window, async move |this, cx| {
            let hint = work.await;
            this.update_in(cx, |ws, window, cx| match hint {
                Some(hint) => ws.run_install_hint(hint, window, cx),
                None => ws.show_toast(
                    format!(
                        "No install command is known for {server}: install it, then restart it"
                    ),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn run_install_hint(
        &mut self,
        hint: InstallHint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match hint {
            InstallHint::Link(url) => cx.open_url(url),
            InstallHint::Command(command) => self.start_install(command, window, cx),
        }
    }

    /// Runs `command` in a new terminal tab (no shell) and shows it; when
    /// it exits the bridge retries the servers that were not installed.
    fn start_install(
        &mut self,
        command: InstallCommandLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = command.display();
        if let Some(running) = self.lsp.read(cx).installing() {
            self.show_toast(format!("`{running}` is still running"), cx);
            return;
        }
        self.lsp
            .update(cx, |b, cx| b.install_started(display.clone(), cx));
        // Tests check what would run without starting it.
        if cfg!(test) {
            let _ = window;
            return;
        }
        // The full path, so a `.cmd` shim is found on Windows.
        let program = find_executable(&command.program)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(command.program);
        let cwd = self
            .project_root()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let package = PackageCommand {
            program,
            args: command.args,
            cwd,
            display: display.clone(),
        };
        let lsp = self.lsp.downgrade();
        let on_exit: crate::terminal_panel::OnExit = Box::new(move |code, cx| {
            lsp.update(cx, |b, cx| b.install_exited(code, cx)).ok();
        });
        let terminals = self.bottom_panel.read(cx).terminals().clone();
        let started = terminals.update(cx, |t, cx| {
            t.run_program(&package, Some(on_exit), window, cx)
        });
        match started {
            Ok(()) => {
                self.bottom_panel
                    .update(cx, |p, cx| p.set_active_tab(BottomTab::Terminal, cx));
                self.bottom_panel_visible = true;
            }
            Err(err) => {
                self.lsp.update(cx, |b, cx| b.install_exited(None, cx));
                self.show_toast(format!("Cannot run {display}: {err}"), cx);
            }
        }
        cx.notify();
    }

    /// Lists the servers that the open project's files (and the open
    /// files) need and that are not installed, to install one.
    pub fn show_missing_language_servers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = settings(cx).clone();
        let root = self.project_root().map(Path::to_path_buf);
        let mut paths: Vec<PathBuf> = self
            .editors(cx)
            .iter()
            .filter_map(|e| e.read(cx).path().map(Path::to_path_buf))
            .collect();
        if let Some(root) = &root {
            paths.extend(self.project_files.iter().map(|f| root.join(f)));
        }
        let work = cx.background_spawn(async move {
            let languages = languages_of(&paths, &config);
            missing_servers(&servers_of(&languages, &config), root.as_deref())
        });
        cx.spawn_in(window, async move |this, cx| {
            let missing = work.await;
            this.update_in(cx, |ws, window, cx| {
                ws.missing_servers_found(missing, window, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn missing_servers_found(
        &mut self,
        missing: Vec<(String, Option<InstallHint>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if missing.is_empty() {
            self.show_toast_as(
                "Every language server this project uses is installed",
                ToastLevel::Info,
                cx,
            );
            return;
        }
        let items = missing
            .iter()
            .map(|(name, hint)| {
                let detail = match hint {
                    Some(InstallHint::Command(c)) => c.display(),
                    Some(InstallHint::Link(url)) => format!("how to install: {url}"),
                    None => "not installed".to_string(),
                };
                PickItem::new(name.clone(), detail)
            })
            .collect();
        let servers = missing.into_iter().map(|(name, _)| name).collect();
        self.show_list_picker(
            "Install a missing language server…",
            IconName::Package,
            items,
            PickPurpose::InstallServer { servers },
            window,
            cx,
        );
    }
}

/// The language ids of `paths` (one look per file extension).
fn languages_of(paths: &[PathBuf], config: &arcode_config::Config) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut languages = BTreeSet::new();
    for path in paths {
        let key = match path.extension() {
            Some(ext) => ext.to_string_lossy().to_lowercase(),
            None => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        if seen.insert(key) {
            languages.insert(language_for(path, config).id.to_lowercase());
        }
    }
    languages.into_iter().collect()
}
