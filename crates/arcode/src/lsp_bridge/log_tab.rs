//! Language server commands of the palette ("LSP: Restart Language
//! Servers", "LSP: Show Language Server Log") and the read-only tab that
//! shows a server's log, live: new lines are appended while it is open.

use arcode_buffer::{Buffer, Point};
use arcode_editor::{Editor, ScrollAnchor};
use gpui::{App, AppContext, Context, Window, actions};

use crate::pane::TabLabel;
use crate::workspace::Workspace;

actions!(
    lsp,
    [
        /// Restarts the language servers of the active file.
        RestartLanguageServers,
        /// Opens the logs of the active file's language servers.
        ShowLanguageServerLog,
    ]
);

/// Registers the commands (handled app-wide for the active window).
pub fn init(cx: &mut App) {
    cx.on_action(|_: &RestartLanguageServers, cx| {
        with_active_workspace(cx, |ws, _, cx| {
            if ws.lsp().read(cx).server_entries(cx).is_empty() {
                ws.show_toast("No language server is running for this file", cx);
                return;
            }
            ws.lsp().clone().update(cx, |b, cx| b.restart_active(cx));
        })
    });
    cx.on_action(|_: &ShowLanguageServerLog, cx| {
        with_active_workspace(cx, |ws, window, cx| {
            let entries = ws.lsp().read(cx).server_entries(cx);
            if entries.is_empty() {
                ws.show_toast("No language server is running for this file", cx);
            }
            for entry in entries.iter().rev() {
                open_server_log(ws, &entry.name, window, cx);
            }
        })
    });
    crate::commands::register_global(
        cx,
        "LSP: Restart Language Servers",
        Box::new(RestartLanguageServers),
    );
    crate::commands::register_global(
        cx,
        "LSP: Show Language Server Log",
        Box::new(ShowLanguageServerLog),
    );
}

/// Runs `f` on the active window's workspace, after the current action
/// (an action runs while its window is updated).
fn with_active_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    let handle = cx
        .active_window()
        .and_then(|w| w.downcast::<Workspace>())
        .or_else(|| crate::windows::workspace_windows(cx).first().copied());
    let Some(handle) = handle else {
        return;
    };
    cx.defer(move |cx| {
        handle.update(cx, |ws, window, cx| f(ws, window, cx)).ok();
    });
}

/// The tab label of `server`'s log.
pub fn log_label(server: &str) -> TabLabel {
    TabLabel {
        title: format!("{server} log").into(),
        key: format!("lsp-log\u{0}{server}").into(),
        tooltip: format!("Language server log: {server}").into(),
    }
}

/// Opens `server`'s log in a read-only tab of the active pane (replacing
/// an open one), scrolled to the end. Lines logged later are appended; a
/// cursor left at the end follows them.
pub fn open_server_log(
    ws: &mut Workspace,
    server: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let bridge = ws.lsp().clone();
    let (text, seen) = bridge.read(cx).server_log_since(server, 0);
    let feed = bridge.read(cx).log_feed();
    let label = log_label(server);
    for pane in ws.panes().to_vec() {
        if let Some(ix) = pane.read(cx).position_of_key(&label.key) {
            pane.update(cx, |p, cx| p.close(ix, window, cx));
        }
    }
    // No path: the tab can never be saved over a file.
    let server = server.to_string();
    let bridge = bridge.downgrade();
    let editor = cx.new(|cx| {
        let mut editor = Editor::new(Buffer::new(&text), cx);
        editor.set_read_only(true, cx);
        let last = editor.buffer().line_count().saturating_sub(1);
        editor.go_to(Point::new(last, 0), window, cx);
        editor.scroll_to_line(last, ScrollAnchor::Fit, cx);
        let mut seen = seen;
        // Ends with the editor (an observer whose entity is gone is dropped).
        cx.observe(&feed, move |editor: &mut Editor, _, cx| {
            let Some(bridge) = bridge.upgrade() else {
                return;
            };
            let (new, total) = bridge.read(cx).server_log_since(&server, seen);
            seen = total;
            editor.append_output(&new, cx);
        })
        .detach();
        editor
    });
    ws.active_pane().clone().update(cx, |pane, cx| {
        pane.add_titled_editor(editor, label, window, cx)
    });
    cx.notify();
}
