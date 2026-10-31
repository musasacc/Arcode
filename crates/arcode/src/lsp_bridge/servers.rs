//! What the UI shows and does per language server: status entries for the
//! status bar, restarting, logs, the one-time "not installed" hint and
//! applying a changed configuration while servers run.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use arcode_editor::Editor;
use arcode_lsp::ServerStatus;
use arcode_ui_kit::settings;
use gpui::{App, Context, Entity, SharedString};

use super::convert::{self, ServerState};
use super::{LspBridge, LspBridgeEvent};

/// Lines kept per server log; older ones are dropped.
pub const LOG_LINES: usize = 2000;

/// The last [`LOG_LINES`] lines a server logged (stderr, `logMessage`) plus
/// its status changes.
#[derive(Debug, Default, Clone)]
pub struct ServerLog {
    lines: VecDeque<String>,
    /// Lines pushed so far, dropped ones included.
    total: u64,
}

impl ServerLog {
    /// Appends `line`, dropping the oldest beyond [`LOG_LINES`].
    pub fn push(&mut self, line: impl Into<String>) {
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line.into());
        self.total += 1;
    }

    /// The log as text, one line per entry.
    pub fn text(&self) -> String {
        self.since(0).0
    }

    /// The lines pushed after the first `seen` (those still kept) as text,
    /// and the new count to pass next time. A `seen` beyond the count (the
    /// log was cleared) starts over.
    pub fn since(&self, seen: u64) -> (String, u64) {
        let first = self.total - self.lines.len() as u64;
        let skip = if seen > self.total {
            0
        } else {
            seen.saturating_sub(first) as usize
        };
        let mut out = String::new();
        for line in self.lines.iter().skip(skip) {
            out.push_str(line);
            out.push('\n');
        }
        (out, self.total)
    }
}

/// Notified (once per batch of server events) when a server log grew;
/// open log tabs observe it.
pub struct LogFeed;

/// How a server entry is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Initialized and idle.
    Ready,
    /// Starting or reporting progress.
    Working,
    /// Shut down.
    Stopped,
    /// The executable was not found.
    NotInstalled,
    /// Crashed or could not start.
    Failed,
}

/// One server of the active file's language, as the status bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    /// Server name (the `[lsp.<name>]` key).
    pub name: SharedString,
    /// State, e.g. `ready` or `indexing 40%`.
    pub state: SharedString,
    /// For the icon and color.
    pub kind: EntryKind,
}

/// The entry for `name` in `state`.
pub fn entry(name: &str, state: &ServerState) -> ServerEntry {
    let kind = match &state.status {
        ServerStatus::Running if state.progress.is_empty() => EntryKind::Ready,
        ServerStatus::Running | ServerStatus::Busy(_) | ServerStatus::Starting => {
            EntryKind::Working
        }
        ServerStatus::Stopped => EntryKind::Stopped,
        ServerStatus::Failed(reason) if convert::is_not_installed(reason) => {
            EntryKind::NotInstalled
        }
        ServerStatus::Failed(_) => EntryKind::Failed,
    };
    ServerEntry {
        name: name.to_string().into(),
        state: convert::state_text(state).into(),
        kind,
    }
}

/// The command line configured for `server`, e.g. `typos-lsp --stdio`.
pub fn command_line(server: &str, cx: &App) -> String {
    let config = settings(cx);
    let Some(s) = config.lsp_server(server) else {
        return server.to_string();
    };
    std::iter::once(s.command.as_str())
        .chain(s.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

impl LspBridge {
    /// The language of the active editor (status bar entries follow it).
    pub fn set_active_language(&mut self, language: Option<String>, cx: &mut Context<Self>) {
        if self.active_language != language {
            self.active_language = language;
            cx.notify();
        }
    }

    /// The servers of the active editor's language that were started, in
    /// priority order.
    pub fn server_entries(&self, cx: &App) -> Vec<ServerEntry> {
        let Some(language) = self.active_language.as_deref() else {
            return Vec::new();
        };
        let Some(language) = settings(cx).language(language) else {
            return Vec::new();
        };
        let mut entries: Vec<ServerEntry> = Vec::new();
        for name in &language.servers {
            // A server listed twice serves the file once.
            if entries.iter().any(|e| e.name == name.as_str()) {
                continue;
            }
            if let Some(state) = self.servers.get(name) {
                entries.push(entry(name, state));
            }
        }
        entries
    }

    /// What `server` logged, oldest first.
    pub fn server_log(&self, server: &str) -> String {
        self.logs
            .get(server)
            .map(ServerLog::text)
            .unwrap_or_default()
    }

    /// What `server` logged after the first `seen` lines; see
    /// [`ServerLog::since`].
    pub fn server_log_since(&self, server: &str, seen: u64) -> (String, u64) {
        self.logs
            .get(server)
            .map(|log| log.since(seen))
            .unwrap_or_default()
    }

    /// Notified when a server log grew.
    pub fn log_feed(&self) -> Entity<LogFeed> {
        self.log_feed.clone()
    }

    /// Appends a line to `server`'s log; [`LspBridge::flush_logs`] tells
    /// the open log tabs.
    pub(super) fn log_line(&mut self, server: &str, line: impl Into<String>) {
        self.logs.entry(server.to_string()).or_default().push(line);
        self.logs_dirty = true;
    }

    /// Notifies the log tabs once if lines were logged since the last call.
    pub(super) fn flush_logs(&mut self, cx: &mut App) {
        if std::mem::take(&mut self.logs_dirty) {
            self.log_feed.update(cx, |_, cx| cx.notify());
        }
    }

    /// Stops `server` and starts it again for the open files that use it.
    /// Its diagnostics are cleared until it reports again.
    pub fn restart_server(&mut self, server: &str, cx: &mut Context<Self>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        self.log_line(server, "--- restart requested");
        self.forget_server(server, cx);
        if !manager.restart_server(server) {
            // Not running (e.g. it failed): open files start it again.
            self.servers.remove(server);
        }
        self.hinted.remove(server);
        self.flush_logs(cx);
        cx.emit(LspBridgeEvent::StatusChanged);
        cx.notify();
    }

    /// Restarts every server of the active editor's language.
    pub fn restart_active(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .server_entries(cx)
            .into_iter()
            .map(|e| e.name.to_string())
            .collect();
        for name in names {
            self.restart_server(&name, cx);
        }
    }

    /// Drops `server`'s diagnostics and shows it as starting.
    fn forget_server(&mut self, server: &str, cx: &mut Context<Self>) {
        self.store.clear_server(server);
        if let Some(state) = self.servers.get_mut(server) {
            *state = ServerState::default();
        }
        let paths: Vec<_> = self.docs.values().map(|d| d.path.clone()).collect();
        for path in paths {
            self.push_diagnostics(&path, cx);
        }
        cx.emit(LspBridgeEvent::DiagnosticsChanged);
    }

    /// Drops what the instance of `server` at `root` that crashed or
    /// stopped reported: all of `server`'s diagnostics when no other
    /// instance of it runs, else those of files under `root`.
    pub(super) fn drop_instance_diagnostics(
        &mut self,
        server: &str,
        root: &Path,
        cx: &mut Context<Self>,
    ) {
        let others = self
            .manager
            .as_ref()
            .is_some_and(|m| m.is_server_running(server));
        let mut paths: Vec<PathBuf> = self
            .store
            .all()
            .filter(|(path, s, _)| *s == server && (!others || path.starts_with(root)))
            .map(|(path, ..)| path.to_path_buf())
            .collect();
        paths.dedup();
        if paths.is_empty() {
            return;
        }
        for path in &paths {
            self.store.update(server, path.clone(), Vec::new());
            self.push_diagnostics(path, cx);
        }
        cx.emit(LspBridgeEvent::DiagnosticsChanged);
    }

    /// Applies the current settings to running servers: only servers whose
    /// configuration changed restart, files follow changed server lists,
    /// and editors whose language gained or lost all servers are
    /// registered again.
    pub(super) fn settings_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(manager) = self.manager.clone() {
            let (servers, languages) = convert::server_configs(settings(cx));
            let configured: Vec<String> = servers.iter().map(|s| s.name.clone()).collect();
            for name in manager.reconfigure(servers, languages) {
                let what = if !configured.contains(&name) {
                    "no longer used by any language, stopped"
                } else if manager.is_server_running(&name) {
                    "configuration changed, restarting"
                } else {
                    "configuration changed, stopped (no open file uses it)"
                };
                log::info!("{name}: {what}");
                self.log_line(&name, format!("--- {what}"));
                self.forget_server(&name, cx);
                self.hinted.remove(&name);
                if !configured.contains(&name) {
                    self.servers.remove(&name);
                }
            }
        }
        let stale: Vec<Entity<Editor>> = self
            .docs
            .values()
            .filter(|d| d.synced != (self.enabled && has_servers(d, cx)))
            .filter_map(|d| d.editor.upgrade())
            .collect();
        for editor in stale {
            self.editor_path_changed(&editor, cx);
        }
        self.flush_logs(cx);
        cx.emit(LspBridgeEvent::StatusChanged);
        cx.notify();
    }

    /// Reports a failed server once per session: the executable missing
    /// gets a hint with the configured command (nothing is installed
    /// automatically); other failures a short message.
    pub(super) fn report_failure(&mut self, server: &str, reason: &str, cx: &mut Context<Self>) {
        if convert::is_not_installed(reason) {
            if self.hinted.insert(server.to_string()) {
                let command = command_line(server, cx);
                cx.emit(LspBridgeEvent::Message(
                    format!(
                        "Language server {server} is not installed (command: `{command}`). \
                         Install it, then restart it from the status bar."
                    )
                    .into(),
                ));
            }
            return;
        }
        let reason = convert::first_line(reason);
        cx.emit(LspBridgeEvent::Message(
            format!("{server} failed: {reason}").into(),
        ));
    }
}

/// Whether the editor of `doc` has a language with servers now.
fn has_servers(doc: &super::Doc, cx: &App) -> bool {
    doc.editor.upgrade().is_some_and(|editor| {
        let language = editor.read(cx).language().id.to_lowercase();
        settings(cx)
            .language(&language)
            .is_some_and(|l| !l.servers.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_keeps_the_newest_lines() {
        let mut log = ServerLog::default();
        for i in 0..LOG_LINES + 5 {
            log.push(format!("line {i}"));
        }
        let text = log.text();
        assert!(text.starts_with("line 5\n"));
        assert!(text.ends_with(&format!("line {}\n", LOG_LINES + 4)));
        let total = (LOG_LINES + 5) as u64;
        assert_eq!(
            log.since(total - 1),
            (format!("line {}\n", LOG_LINES + 4), total)
        );
        assert_eq!(log.since(total).0, "");
        // Lines dropped since: what is kept; a cleared log: from the start.
        assert_eq!(log.since(2).0, text);
        assert_eq!(log.since(total + 7).0, text);
    }

    #[test]
    fn entries_by_state() {
        let mut state = ServerState::default();
        assert_eq!(entry("ra", &state).kind, EntryKind::Working);
        state.status = ServerStatus::Running;
        let e = entry("ra", &state);
        assert_eq!((e.kind, e.state.as_ref()), (EntryKind::Ready, "ready"));
        state.status = ServerStatus::Failed("typos-lsp not found on PATH (install it)".into());
        let e = entry("typos", &state);
        assert_eq!(
            (e.kind, e.state.as_ref()),
            (EntryKind::NotInstalled, "not installed")
        );
        state.status = ServerStatus::Failed("exit code 1".into());
        assert_eq!(entry("ra", &state).kind, EntryKind::Failed);
    }
}
