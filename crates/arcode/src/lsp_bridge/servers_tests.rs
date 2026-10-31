//! Several servers per file in the bridge: status entries, logs, the
//! one-time "not installed" hint and the status bar menu.

use std::path::PathBuf;

use arcode_config::Config;
use arcode_lsp::lsp_types::MessageType;
use arcode_lsp::{LspEvent, ServerStatus};
use gpui::{
    AppContext, Entity, Modifiers, SharedString, TestAppContext, VisualTestContext, WeakEntity, px,
};

use super::servers::EntryKind;
use super::status_view::LspStatusView;
use super::{LspBridge, LspBridgeEvent};

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<LspBridge>,
    std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) {
    crate::test_support::init(cx);
    cx.update(|cx| {
        let (config, warnings) = Config::from_toml_str(
            "[language.rust]\nservers = [\"rust-analyzer\", \"typos\", \"rust-analyzer\"]\n\
             [lsp.typos]\ncommand = \"typos-lsp\"\nargs = [\"--stdio\"]\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        arcode_ui_kit::set_settings(config, cx);
    });
    let bridge = cx.new(LspBridge::new);
    let messages = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = messages.clone();
    cx.update(|cx| {
        cx.subscribe(&bridge, move |_, event: &LspBridgeEvent, _| {
            if let LspBridgeEvent::Message(text) = event {
                sink.borrow_mut().push(text.to_string());
            }
        })
        .detach();
    });
    (bridge, messages)
}

fn status(server: &str, status: ServerStatus) -> LspEvent {
    LspEvent::ServerStatus {
        server: server.into(),
        root: PathBuf::from("/proj"),
        status,
    }
}

#[gpui::test]
fn every_server_of_the_active_language_is_listed(cx: &mut TestAppContext) {
    let (bridge, messages) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.handle_event(status("rust-analyzer", ServerStatus::Running), cx);
        b.handle_event(status("typos", ServerStatus::Starting), cx);
        // Not the active language yet.
        assert!(b.server_entries(cx).is_empty());
        b.set_active_language(Some("rust".into()), cx);
        let entries: Vec<_> = b
            .server_entries(cx)
            .into_iter()
            .map(|e| (e.name.to_string(), e.state.to_string(), e.kind))
            .collect();
        assert_eq!(
            entries,
            [
                ("rust-analyzer".into(), "ready".into(), EntryKind::Ready),
                ("typos".into(), "starting…".into(), EntryKind::Working),
            ]
        );
        // A missing executable is reported once, with its command.
        let missing = "typos-lsp not found on PATH (install it or set the full path)";
        b.handle_event(status("typos", ServerStatus::Failed(missing.into())), cx);
        b.handle_event(status("typos", ServerStatus::Failed(missing.into())), cx);
        assert_eq!(b.server_entries(cx)[1].kind, EntryKind::NotInstalled);
        b.handle_event(
            LspEvent::Log {
                server: "rust-analyzer".into(),
                level: MessageType::LOG,
                message: "indexing done".into(),
            },
            cx,
        );
        assert!(
            b.server_log("rust-analyzer")
                .contains("[log] indexing done")
        );
        assert!(b.server_log("typos").contains("Failed"));
    });
    cx.run_until_parked();
    let messages = messages.borrow();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("`typos-lsp --stdio`"), "{messages:?}");
}

#[gpui::test]
fn the_status_menu_opens_per_server(cx: &mut TestAppContext) {
    let (bridge, _) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.handle_event(status("rust-analyzer", ServerStatus::Running), cx);
        b.handle_event(status("typos", ServerStatus::Running), cx);
        b.set_active_language(Some("rust".into()), cx);
    });
    let (view, cx) = cx
        .add_window_view(|_, cx| LspStatusView::new(bridge.clone(), WeakEntity::new_invalid(), cx));
    let open = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.open_menu().cloned());
    let toggle = |name: &str, cx: &mut VisualTestContext| {
        let name = SharedString::from(name.to_string());
        cx.update(|window, cx| view.update(cx, |v, cx| v.toggle_menu(name, window, cx)));
    };
    toggle("typos", cx);
    assert_eq!(open(cx), Some("typos".into()));
    toggle("rust-analyzer", cx);
    assert_eq!(open(cx), Some("rust-analyzer".into()));
    // Restarting without a running manager is harmless and closes it.
    cx.update(|window, cx| view.update(cx, |v, cx| v.restart("rust-analyzer", window, cx)));
    assert_eq!(open(cx), None);
    // A menu whose server leaves the list closes.
    toggle("typos", cx);
    bridge.update(cx, |b, cx| b.set_active_language(Some("python".into()), cx));
    cx.run_until_parked();
    assert_eq!(open(cx), None);
}

#[gpui::test]
fn the_status_menu_closes_on_its_entry_and_escape(cx: &mut TestAppContext) {
    let (bridge, _) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.handle_event(status("rust-analyzer", ServerStatus::Running), cx);
        b.set_active_language(Some("rust".into()), cx);
    });
    let (view, cx) = cx
        .add_window_view(|_, cx| LspStatusView::new(bridge.clone(), WeakEntity::new_invalid(), cx));
    let open = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.open_menu().cloned());
    cx.run_until_parked();
    let entry = cx
        .debug_bounds("lsp-server-rust-analyzer")
        .expect("the entry is drawn")
        .center();
    cx.simulate_click(entry, Modifiers::none());
    assert_eq!(open(cx), Some("rust-analyzer".into()));
    // Clicking the entry again closes it (the press outside the menu
    // closes it first; the entry must not open it again).
    cx.simulate_click(entry, Modifiers::none());
    assert_eq!(open(cx), None);
    cx.simulate_click(entry, Modifiers::none());
    assert_eq!(open(cx), Some("rust-analyzer".into()));
    cx.simulate_keystrokes("escape");
    assert_eq!(open(cx), None);
    // A click elsewhere closes it, and the entry still opens it after.
    cx.simulate_click(entry, Modifiers::none());
    cx.simulate_click(gpui::point(px(700.), px(500.)), Modifiers::none());
    assert_eq!(open(cx), None);
    cx.simulate_click(entry, Modifiers::none());
    assert_eq!(open(cx), Some("rust-analyzer".into()));
}

#[gpui::test]
fn settings_reload_without_servers_running_is_harmless(cx: &mut TestAppContext) {
    let (bridge, _) = setup(cx);
    cx.update(|cx| arcode_ui_kit::set_settings(Config::default(), cx));
    cx.run_until_parked();
    bridge.read_with(cx, |b, cx| assert!(b.server_entries(cx).is_empty()));
}

#[gpui::test]
fn a_crashed_server_drops_its_diagnostics(cx: &mut TestAppContext) {
    use arcode_lsp::lsp_types::{Diagnostic, Range};
    let (bridge, _) = setup(cx);
    let path = PathBuf::from("/proj/src/main.rs");
    let diagnostic = |message: &str| Diagnostic {
        range: Range::default(),
        message: message.into(),
        ..Default::default()
    };
    bridge.update(cx, |b, cx| {
        for server in ["rust-analyzer", "typos"] {
            b.handle_event(
                LspEvent::Diagnostics {
                    server: server.into(),
                    path: path.clone(),
                    version: None,
                    diagnostics: vec![diagnostic(server)],
                },
                cx,
            );
        }
        assert_eq!(b.store().for_path(&path).len(), 2);
        let reason = "server closed the connection";
        b.handle_event(status("typos", ServerStatus::Failed(reason.into())), cx);
        let left: Vec<_> = b
            .store()
            .for_path(&path)
            .into_iter()
            .map(|(s, _)| s.to_string())
            .collect();
        assert_eq!(left, ["rust-analyzer"]);
        b.handle_event(status("rust-analyzer", ServerStatus::Stopped), cx);
        assert!(b.store().is_empty());
    });
}

#[gpui::test]
fn log_tabs_are_told_once_per_batch(cx: &mut TestAppContext) {
    let (bridge, _) = setup(cx);
    let feed = bridge.read_with(cx, |b, _| b.log_feed());
    let told = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = told.clone();
    cx.update(|cx| {
        cx.observe(&feed, move |_, _| count.set(count.get() + 1))
            .detach()
    });
    bridge.update(cx, |b, cx| {
        for line in ["a", "b", "c"] {
            b.handle_event(
                LspEvent::Log {
                    server: "typos".into(),
                    level: MessageType::LOG,
                    message: line.into(),
                },
                cx,
            );
        }
        b.flush_logs(cx);
        b.flush_logs(cx);
        assert_eq!(b.server_log_since("typos", 1).0, "[log] b\n[log] c\n");
    });
    cx.run_until_parked();
    assert_eq!(told.get(), 1);
}
