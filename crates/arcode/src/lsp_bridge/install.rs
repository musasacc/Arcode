//! Installing a missing language server, only when the user asks for it.
//!
//! A server that is not installed is reported once, with a button: Install
//! when `arcode_project::server_install` has a command for this machine
//! (chosen by what is on `PATH`), else How to Install (its homepage). The
//! server's status bar menu, the Languages page and "LSP: Show Missing
//! Language Servers" offer the same. Install opens a terminal tab running
//! the command, where the user sees what it does; when it exits with
//! success, the servers that were not installed start again (and the
//! bundle is looked for again), without a restart.

use std::path::{Path, PathBuf};

use arcode_config::Config;
use arcode_project::Os;
use arcode_project::packs::find_executable;
use arcode_project::server_install::{InstallHint, install_hint};
use arcode_ui_kit::settings;
use gpui::{Action, App, AppContext as _, Context, SharedString, actions};

use super::log_tab::with_active_workspace;
use super::servers::{EntryKind, command_line, entry};
use super::{LspBridge, LspBridgeEvent, bundled, convert};

/// A server reported as not installed, with what its button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotInstalled {
    /// Server name (the `[lsp.<name>]` key).
    pub server: SharedString,
    /// What to tell the user.
    pub message: SharedString,
    /// The button: Install or How to Install.
    pub label: SharedString,
}

/// Installs a missing server: runs its install command in a terminal tab,
/// or opens the page that says how.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = lsp, no_json)]
pub struct InstallLanguageServer {
    /// Server name (the `[lsp.<name>]` key).
    pub server: SharedString,
}

actions!(
    lsp,
    [
        /// Lists the language servers the open project's files need that
        /// are not installed.
        ShowMissingLanguageServers,
    ]
);

/// Registers the install commands (handled app-wide for the active
/// window).
pub fn init(cx: &mut App) {
    cx.on_action(|action: &InstallLanguageServer, cx| {
        let server = action.server.to_string();
        with_active_workspace(cx, move |ws, window, cx| {
            ws.install_language_server(&server, window, cx)
        })
    });
    cx.on_action(|_: &ShowMissingLanguageServers, cx| {
        with_active_workspace(cx, |ws, window, cx| {
            ws.show_missing_language_servers(window, cx)
        })
    });
    crate::commands::register_global(
        cx,
        "LSP: Show Missing Language Servers",
        Box::new(ShowMissingLanguageServers),
    );
}

/// The button that goes with `hint`.
pub fn button_label(hint: &InstallHint) -> &'static str {
    match hint {
        InstallHint::Command(_) => "Install",
        InstallHint::Link(_) => "How to Install",
    }
}

/// How to install server `command` on this machine (walks `PATH`: off
/// the UI thread).
pub fn hint_for(command: &str) -> Option<InstallHint> {
    install_hint(command, Os::current(), &|program| {
        find_executable(program).is_some()
    })
}

/// The command `server` runs (its configured command, else its name).
pub fn server_command(server: &str, config: &Config) -> String {
    config
        .lsp_server(server)
        .map(|s| s.command.clone())
        .unwrap_or_else(|| server.to_string())
}

/// What to say about `server`, not installed for `reason`, whose command
/// line is `line`.
pub fn not_installed_message(
    server: &str,
    line: &str,
    reason: &str,
    hint: Option<&InstallHint>,
) -> String {
    // A rustup placeholder: the reason names the toolchain and the fix.
    if reason.contains("in the Rust toolchain") {
        return format!("Language server {}", convert::first_line(reason));
    }
    let head = format!("Language server {server} is not installed (command: `{line}`).");
    match hint {
        Some(InstallHint::Command(command)) => {
            format!("{head} Install runs `{}` in a terminal.", command.display())
        }
        Some(InstallHint::Link(_)) | None => {
            format!("{head} Install it, then restart it from the status bar.")
        }
    }
}

impl LspBridge {
    /// Reports a failed server: a bundled server whose runtime is missing
    /// or too old says what it needs, once per session; so does a server
    /// that is not installed, with an install button when there is a way
    /// for this machine (nothing is installed until it is clicked); other
    /// failures get a short message.
    pub(super) fn report_failure(&mut self, server: &str, reason: &str, cx: &mut Context<Self>) {
        let note = self
            .runtime_notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(server)
            .cloned();
        if let Some(note) = note {
            if self.hinted.insert(server.to_string()) {
                cx.emit(LspBridgeEvent::Message(note.into()));
            }
            return;
        }
        if convert::is_not_installed(reason) {
            if self.hinted.insert(server.to_string()) {
                self.report_not_installed(server, reason, cx);
            }
            return;
        }
        let reason = convert::first_line(reason);
        cx.emit(LspBridgeEvent::Message(
            format!("{server} failed: {reason}").into(),
        ));
    }

    /// Finds how to install `server` in the background, then reports it.
    fn report_not_installed(&mut self, server: &str, reason: &str, cx: &mut Context<Self>) {
        let command = server_command(server, settings(cx));
        let line = command_line(server, cx);
        let (server, reason) = (server.to_string(), reason.to_string());
        let work = cx.background_spawn(async move { hint_for(&command) });
        cx.spawn(async move |this, cx| {
            let hint = work.await;
            this.update(cx, |bridge, cx| {
                let message = not_installed_message(&server, &line, &reason, hint.as_ref());
                match &hint {
                    Some(hint) => cx.emit(LspBridgeEvent::NotInstalled(NotInstalled {
                        server: server.clone().into(),
                        message: message.into(),
                        label: button_label(hint).into(),
                    })),
                    None => cx.emit(LspBridgeEvent::Message(message.into())),
                }
                bridge.install_hints.insert(server, hint);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// How to install `server`, once it was reported as not installed.
    pub fn install_hint(&self, server: &str) -> Option<&InstallHint> {
        self.install_hints.get(server).and_then(Option::as_ref)
    }

    /// The servers that failed because they are not installed, sorted.
    pub fn not_installed(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .servers
            .iter()
            .filter(|(name, state)| entry(name, state).kind == EntryKind::NotInstalled)
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    /// The install command running in a terminal, if any.
    pub fn installing(&self) -> Option<&str> {
        self.installing.as_deref()
    }

    /// Records that `command` runs in a terminal now.
    pub fn install_started(&mut self, command: String, cx: &mut Context<Self>) {
        self.installing = Some(command);
        cx.notify();
    }

    /// The install command exited with `code`: on success the servers that
    /// were not installed start again; else the user is told.
    pub fn install_exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        let command = self.installing.take().unwrap_or_default();
        if code != Some(0) {
            // `None`: it was killed, or its tab was closed while it ran.
            let message = match code {
                Some(code) => {
                    format!("`{command}` did not finish (exit code {code}): see its terminal tab")
                }
                None => format!("`{command}` was stopped before it finished"),
            };
            cx.emit(LspBridgeEvent::Message(message.into()));
            cx.notify();
            return;
        }
        let retried = self.retry_not_installed(&command, cx);
        let message = if retried.is_empty() {
            format!("`{command}` finished")
        } else {
            format!(
                "`{command}` finished: starting {} again",
                retried.join(", ")
            )
        };
        cx.emit(LspBridgeEvent::Message(message.into()));
    }

    /// Starts every server that was not installed again (an install may
    /// have put it on `PATH`, or fetched the bundle). Returns their names.
    /// The servers that `command` installs are reported again if they are
    /// still missing; the others stay quiet.
    pub fn retry_not_installed(&mut self, command: &str, cx: &mut Context<Self>) -> Vec<String> {
        bundled::recheck_bundle();
        let names = self.not_installed();
        for name in &names {
            log::info!("{name}: starting again after an install");
            self.log_line(name, "--- install finished, starting again");
            let installed_by = self
                .install_hint(name)
                .is_some_and(|hint| hint.display() == command);
            if installed_by {
                self.install_hints.remove(name);
                self.hinted.remove(name);
            }
            self.restart_quietly(name, cx);
        }
        names
    }

    /// Sets how `server` installs (tests).
    #[cfg(test)]
    pub(crate) fn set_install_hint(&mut self, server: &str, hint: Option<InstallHint>) {
        self.install_hints.insert(server.to_string(), hint);
    }
}

/// The servers (name, command) the languages in `languages` use.
pub fn servers_of(languages: &[String], config: &Config) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for language in languages {
        let Some(settings) = config.language(language) else {
            continue;
        };
        for name in settings.configured_servers() {
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.clone(), server_command(name, config)));
            }
        }
    }
    out
}

/// Which of `servers` (name, command) are missing here: not a file that
/// exists, not installed (a rustup placeholder does not count) and not
/// bundled; each with how to install it. Blocks: call off the UI thread.
pub fn missing_servers(
    servers: &[(String, String)],
    root: Option<&Path>,
) -> Vec<(String, Option<InstallHint>)> {
    let cx = bundled::launch_context();
    servers
        .iter()
        .filter(|(_, command)| {
            if command.contains(['/', '\\']) || Path::new(command).is_absolute() {
                return !PathBuf::from(command).is_file();
            }
            !matches!(
                bundled::find_server(command, root),
                bundled::Found::Installed(_)
            ) && bundled::launch(command, &cx).is_none()
        })
        .map(|(name, command)| (name.clone(), hint_for(command)))
        .collect()
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
