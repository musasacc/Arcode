//! The "not installed" report, its install button and the retry after an
//! install (no real server or install command runs).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use arcode_config::Config;
use arcode_lsp::{LspEvent, ServerStatus};
use arcode_project::server_install::{InstallCommandLine, InstallHint};
use gpui::{AppContext, Entity, SharedString, TestAppContext, WeakEntity};

use super::*;
use crate::lsp_bridge::status_view::LspStatusView;

fn setup(cx: &mut TestAppContext) -> (Entity<LspBridge>, Rc<RefCell<Vec<LspBridgeEvent>>>) {
    crate::test_support::init(cx);
    cx.update(|cx| {
        let (config, warnings) = Config::from_toml_str(
            "[language.rust]\nservers = [\"rust-analyzer\", \"typos\"]\n\
             [lsp.typos]\ncommand = \"typos-lsp\"\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        arcode_ui_kit::set_settings(config, cx);
    });
    let bridge = cx.new(LspBridge::new);
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&bridge, move |_, event: &LspBridgeEvent, _| {
            sink.borrow_mut().push(event.clone());
        })
        .detach();
    });
    (bridge, events)
}

fn failed(server: &str, reason: &str) -> LspEvent {
    LspEvent::ServerStatus {
        server: server.into(),
        root: PathBuf::from("/proj"),
        status: ServerStatus::Failed(reason.into()),
    }
}

const PLACEHOLDER: &str = "rust-analyzer is not installed in the Rust toolchain \
     1.97.0-x86_64-unknown-linux-gnu: run `rustup component add rust-analyzer`";

fn reports(events: &Rc<RefCell<Vec<LspBridgeEvent>>>) -> Vec<NotInstalled> {
    events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            LspBridgeEvent::NotInstalled(report) => Some(report.clone()),
            _ => None,
        })
        .collect()
}

fn messages(events: &Rc<RefCell<Vec<LspBridgeEvent>>>) -> Vec<String> {
    events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            LspBridgeEvent::Message(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn messages_say_what_install_runs() {
    let npm = InstallHint::Command(InstallCommandLine {
        program: "npm".into(),
        args: vec!["install".into(), "-g".into(), "pyright".into()],
    });
    let text = not_installed_message(
        "pyright-langserver",
        "pyright-langserver --stdio",
        "pyright-langserver not found on PATH (install it or set the full path)",
        Some(&npm),
    );
    assert_eq!(
        text,
        "Language server pyright-langserver is not installed (command: \
         `pyright-langserver --stdio`). Install runs `npm install -g pyright` in a terminal."
    );
    assert_eq!(button_label(&npm), "Install");
    let link = InstallHint::Link("https://clangd.llvm.org/installation");
    assert_eq!(button_label(&link), "How to Install");
    // A rustup placeholder: the toolchain and the fix.
    let text = not_installed_message("rust-analyzer", "rust-analyzer", PLACEHOLDER, None);
    assert_eq!(text, format!("Language server {PLACEHOLDER}"));
}

#[gpui::test]
fn a_rustup_placeholder_is_reported_once_with_its_fix(cx: &mut TestAppContext) {
    let (bridge, events) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.set_active_language(Some("rust".into()), cx);
        b.handle_event(failed("rust-analyzer", PLACEHOLDER), cx);
        b.handle_event(failed("rust-analyzer", PLACEHOLDER), cx);
        assert_eq!(b.server_entries(cx)[0].kind, EntryKind::NotInstalled);
        assert_eq!(b.server_entries(cx)[0].state.as_ref(), "not installed");
        assert_eq!(b.not_installed(), ["rust-analyzer"]);
    });
    cx.run_until_parked();
    // rustup may or may not be on this machine's PATH: Install when it
    // is, else How to Install; the message is the same.
    let shown: Vec<String> = reports(&events)
        .into_iter()
        .map(|r| r.message.to_string())
        .chain(messages(&events))
        .collect();
    assert_eq!(shown, [format!("Language server {PLACEHOLDER}")]);
    for report in reports(&events) {
        assert_eq!(report.server.as_ref(), "rust-analyzer");
        assert!(["Install", "How to Install"].contains(&report.label.as_ref()));
    }
    let hint = bridge.read_with(cx, |b, _| b.install_hint("rust-analyzer").cloned());
    match hint.expect("a way to install it") {
        InstallHint::Command(c) => assert_eq!(c.display(), "rustup component add rust-analyzer"),
        InstallHint::Link(url) => assert!(url.contains("rust-analyzer")),
    }
}

#[gpui::test]
fn an_unknown_server_gets_a_plain_message(cx: &mut TestAppContext) {
    let (bridge, events) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.handle_event(
            failed("typos", "typos-lsp not found on PATH (install it)"),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(reports(&events).is_empty());
    let shown = messages(&events);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(shown[0].contains("(command: `typos-lsp`)"), "{shown:?}");
    assert!(bridge.read_with(cx, |b, _| b.install_hint("typos").is_none()));
}

#[gpui::test]
fn a_successful_install_starts_missing_servers_again(cx: &mut TestAppContext) {
    let (bridge, events) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.handle_event(failed("rust-analyzer", PLACEHOLDER), cx);
        b.handle_event(
            failed("typos", "typos-lsp not found on PATH (install it)"),
            cx,
        );
        b.install_started("rustup component add rust-analyzer".into(), cx);
        assert_eq!(b.installing(), Some("rustup component add rust-analyzer"));
    });
    cx.run_until_parked();
    let rustup = InstallHint::Command(InstallCommandLine {
        program: "rustup".into(),
        args: vec!["component".into(), "add".into(), "rust-analyzer".into()],
    });
    bridge.update(cx, |b, _| {
        b.set_install_hint("rust-analyzer", Some(rustup.clone()));
        b.set_install_hint("typos", None);
    });
    events.borrow_mut().clear();
    // A failed install changes nothing.
    bridge.update(cx, |b, cx| {
        b.install_exited(Some(1), cx);
        assert_eq!(b.installing(), None);
        assert_eq!(b.not_installed(), ["rust-analyzer", "typos"]);
    });
    assert_eq!(
        messages(&events),
        ["`rustup component add rust-analyzer` did not finish (exit code 1): see its terminal tab"]
    );
    // A stopped install (its tab closed while it ran) frees the next one.
    events.borrow_mut().clear();
    bridge.update(cx, |b, cx| {
        b.install_started("rustup component add rust-analyzer".into(), cx);
        b.install_exited(None, cx);
        assert_eq!(b.installing(), None);
    });
    assert_eq!(
        messages(&events),
        ["`rustup component add rust-analyzer` was stopped before it finished"]
    );
    events.borrow_mut().clear();
    bridge.update(cx, |b, cx| {
        b.install_started("rustup component add rust-analyzer".into(), cx);
        b.install_exited(Some(0), cx);
        assert_eq!(b.installing(), None);
        for server in ["rust-analyzer", "typos"] {
            assert!(
                b.server_log(server)
                    .contains("--- install finished, starting again"),
                "{server}"
            );
        }
        // What it installed is looked up again; the rest keeps its button.
        assert_eq!(b.install_hint("rust-analyzer"), None);
    });
    assert_eq!(
        messages(&events),
        ["`rustup component add rust-analyzer` finished: starting rust-analyzer, typos again"]
    );
    // Still missing after the retry: what it installed is reported again,
    // the other server stays quiet.
    events.borrow_mut().clear();
    bridge.update(cx, |b, cx| {
        b.handle_event(failed("rust-analyzer", PLACEHOLDER), cx);
        b.handle_event(
            failed("typos", "typos-lsp not found on PATH (install it)"),
            cx,
        );
    });
    cx.run_until_parked();
    let again: Vec<String> = reports(&events)
        .into_iter()
        .map(|r| r.message.to_string())
        .chain(messages(&events))
        .collect();
    assert_eq!(again, [format!("Language server {PLACEHOLDER}")]);
}

#[gpui::test]
fn the_status_menu_offers_install_for_a_missing_server(cx: &mut TestAppContext) {
    let (bridge, _) = setup(cx);
    bridge.update(cx, |b, cx| {
        b.set_active_language(Some("rust".into()), cx);
        b.handle_event(failed("rust-analyzer", PLACEHOLDER), cx);
        b.handle_event(
            LspEvent::ServerStatus {
                server: "typos".into(),
                root: PathBuf::from("/proj"),
                status: ServerStatus::Running,
            },
            cx,
        );
    });
    cx.run_until_parked();
    let (view, cx) = cx
        .add_window_view(|_, cx| LspStatusView::new(bridge.clone(), WeakEntity::new_invalid(), cx));
    let toggle = |name: &str, cx: &mut gpui::VisualTestContext| {
        let name = SharedString::from(name.to_string());
        cx.update(|window, cx| view.update(cx, |v, cx| v.toggle_menu(name, window, cx)));
        cx.run_until_parked();
    };
    toggle("rust-analyzer", cx);
    assert!(cx.debug_bounds("lsp-menu-install").is_some());
    toggle("typos", cx);
    assert!(cx.debug_bounds("lsp-menu-install").is_none());
}

#[test]
fn every_builtin_server_has_a_way_to_install() {
    let config = Config::default();
    for (name, server) in &config.lsp {
        assert!(
            arcode_project::server_install::homepage(&server.command).is_some(),
            "no install recipe for [lsp.{name}] ({})",
            server.command
        );
    }
}
