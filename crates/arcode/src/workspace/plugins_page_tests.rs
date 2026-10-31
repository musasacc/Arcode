//! The Plugins page and the consent dialog in a workspace: disabling a
//! plugin takes its lenses, panels and commands away; the consent dialog
//! is answered from the keyboard, one prompt at a time, and closes when
//! its call is cancelled; an install that fails says why.

use std::fs;
use std::path::Path;
use std::time::Duration;

use arcode_plugin_host::PluginId;
use arcode_plugin_host::bindings::guest;
use arcode_plugin_host::bridge::EditorBridge;
use arcode_plugin_host::cancel::{CancelToken, ReplyReceiver, oneshot};
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ConsentRequest};
use arcode_plugin_host::grants::GrantScope;
use arcode_ui_kit::IconName;
use gpui::{TestAppContext, VisualTestContext};

use super::tests::{open, project_dir};
use super::*;
use crate::plugins::consent::ConsentAsk;
use crate::plugins::lenses::{LENS_SET, build};
use crate::plugins::page::rows::Status;

const MANIFEST: &str = r#"
[plugin]
id = "acme.tools"
name = "Acme Tools"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"

[capabilities]
editor = "context"

[[contributes.languages]]
id = "http"
name = "HTTP Request"
extensions = ["http"]

[[contributes.commands]]
id = "run"
title = "Acme: Run"

[[contributes.code-lenses]]
languages = ["http"]
"#;

/// A project with `api.http` open and `acme.tools` (manifest only, no
/// engine) bundled.
fn setup(
    cx: &mut TestAppContext,
) -> (
    (tempfile::TempDir, tempfile::TempDir),
    Entity<Workspace>,
    Entity<Editor>,
    &mut VisualTestContext,
) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let http = root.join("api.http");
    fs::write(&http, "GET /a\n").expect("write");
    let plugins = tempfile::tempdir().expect("tempdir");
    let bundled = plugins.path().join("bundled");
    fs::create_dir_all(bundled.join("acme.tools")).expect("mkdir");
    fs::write(bundled.join("acme.tools/plugin.toml"), MANIFEST).expect("write");
    fs::write(bundled.join("acme.tools/plugin.wasm"), b"not wasm").expect("write");
    let base = plugins.path();
    let mut paths = crate::plugins::paths::plugin_paths(
        None,
        &base.join("config"),
        &base.join("data"),
        &base.join("cache"),
        None,
    );
    paths.bundled = vec![bundled];
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        assert!(ws.plugins.client.ensure_with(paths, Vec::new(), false));
        ws.plugins_loaded(window, cx);
        ws.open_file(&http, window, cx);
    });
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&http, cx).expect("editor"));
    ((dir, plugins), ws, editor, cx)
}

fn acme() -> PluginId {
    PluginId::parse("acme.tools").expect("id")
}

#[gpui::test]
fn disabling_a_plugin_takes_what_it_shows_away(cx: &mut TestAppContext) {
    let (_dirs, ws, editor, cx) = setup(cx);
    let lens = guest::CodeLens {
        line: 0,
        title: "Run".into(),
        command: "run".into(),
        args: "null".into(),
    };
    let set = build(vec![(acme(), vec![lens])], 1);
    let key = (acme(), 1);
    ws.update_in(cx, |ws, window, cx| {
        editor.update(cx, |e, cx| e.set_code_lenses(LENS_SET, set.lenses, cx));
        ws.plugin_add_page(&key, "Acme".into(), IconName::Rows, false, window, cx);
        assert_eq!(ws.plugins.contributions.commands.len(), 1);
    });
    let page = ws.read_with(cx, |ws, _| ws.plugins_page().cloned().expect("page"));
    let status = |cx: &mut VisualTestContext| page.read_with(cx, |p, _| p.rows()[0].status);
    // Opening api.http asked it for lenses (its "wasm" does not load).
    assert_ne!(status(cx), Status::Disabled);
    let language = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| {
            let doc = crate::plugins::snapshot::doc_id(&editor);
            ws.plugins
                .client
                .editor()
                .snapshot(doc)
                .expect("published")
                .language
        })
    };
    assert_eq!(language(&ws, cx), "http");

    ws.update_in(cx, |ws, window, cx| {
        ws.plugins_set_enabled(&acme(), false, window, cx)
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert!(
            editor.read(cx).code_lenses(LENS_SET).is_empty(),
            "lenses gone"
        );
        assert!(ws.plugins.panels.is_empty(), "panel closed");
        let page_key = ws.plugins.page_key(&key);
        assert!(
            ws.panes
                .iter()
                .all(|p| p.read(cx).page_position(&page_key).is_none()),
            "its tab is closed"
        );
        assert!(
            ws.plugins.contributions.commands.is_empty(),
            "commands gone"
        );
        assert!(ws.plugins.host_log.iter().any(|l| l.ends_with("disabled")));
    });
    assert_ne!(language(&ws, cx), "http", "its language went with it");
    assert_eq!(status(cx), Status::Disabled);

    ws.update_in(cx, |ws, window, cx| {
        ws.plugins_set_enabled(&acme(), true, window, cx)
    });
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.contributions.commands.len(), 1, "back")
    });
    assert_eq!(language(&ws, cx), "http", "lenses can be asked for again");
    assert_ne!(status(cx), Status::Disabled);
}

fn ask(kind: ConsentKind) -> (ConsentAsk, ReplyReceiver<ConsentAnswer>, CancelToken) {
    let (reply, answer) = oneshot();
    let cancel = CancelToken::new();
    let request = ConsentRequest {
        plugin: acme(),
        plugin_name: "\u{201c}Acme Tools\u{201d}".into(),
        project: None,
        kind,
    };
    let ask = ConsentAsk {
        request,
        cancel: cancel.clone(),
        reply,
    };
    (ask, answer, cancel)
}

fn network() -> ConsentKind {
    ConsentKind::Network {
        method: "GET".into(),
        scheme: "https".into(),
        host: "example.com".into(),
        port: 443,
    }
}

fn answer_of(answer: ReplyReceiver<ConsentAnswer>) -> ConsentAnswer {
    answer
        .wait(&CancelToken::new(), Some(Duration::from_secs(1)))
        .expect("answered")
}

#[gpui::test]
fn the_consent_dialog_is_answered_from_the_keyboard(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (first, first_answer, _) = ask(network());
    let (second, second_answer, _) = ask(network());
    let (third, third_answer, _) = ask(network());
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_ask(first, window, cx);
        ws.plugin_ask(second, window, cx);
        ws.plugin_ask(third, window, cx);
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.prompts_open, 1, "one at a time");
        assert_eq!(ws.plugins.consent_queue.len(), 2);
    });
    // Focus starts on "Allow once"; Tab moves to "Always for this project"
    // (the session here: no project).
    cx.simulate_keystrokes("tab enter");
    cx.run_until_parked();
    assert_eq!(
        answer_of(first_answer),
        ConsentAnswer::Allow {
            scope: GrantScope::Project,
            write: false
        }
    );
    // Esc decides later.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(answer_of(second_answer), ConsentAnswer::Dismissed);
    // Shift+Tab from the first button wraps to Deny.
    cx.simulate_keystrokes("shift-tab enter");
    cx.run_until_parked();
    assert_eq!(answer_of(third_answer), ConsentAnswer::Deny);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.prompts_open, 0);
        assert!(ws.plugins.consent_queue.is_empty());
        let log = ws.plugins.host_log.join("\n");
        assert!(log.contains("https://example.com:443"), "{log}");
        assert!(log.contains("-> allowed for the project"), "{log}");
        assert!(
            log.contains("-> dismissed") && log.contains("-> denied"),
            "{log}"
        );
    });
}

#[gpui::test]
fn the_write_checkbox_is_part_of_the_answer(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (workspace, answer, _) = ask(ConsentKind::Workspace {
        write: true,
        network: false,
    });
    ws.update_in(cx, |ws, window, cx| ws.plugin_ask(workspace, window, cx));
    cx.run_until_parked();
    // The checkbox comes first: Shift+Tab reaches it, Space ticks it.
    cx.simulate_keystrokes("shift-tab space tab right right enter");
    cx.run_until_parked();
    assert_eq!(
        answer_of(answer),
        ConsentAnswer::Allow {
            scope: GrantScope::Global,
            write: true
        }
    );
}

#[gpui::test]
fn a_cancelled_call_closes_its_dialog(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (shown, shown_answer, cancel) = ask(network());
    let (queued, queued_answer, queued_cancel) = ask(network());
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_ask(shown, window, cx);
        ws.plugin_ask(queued, window, cx);
    });
    cx.run_until_parked();
    cancel.cancel();
    queued_cancel.cancel();
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(answer_of(shown_answer), ConsentAnswer::Dismissed);
    let queued = queued_answer.wait(&CancelToken::new(), Some(Duration::from_millis(100)));
    assert!(queued.is_err(), "never shown: {queued:?}");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.prompts_open, 0);
        assert!(ws.plugins.consent_queue.is_empty());
    });
}

#[gpui::test]
fn a_failed_install_says_why(cx: &mut TestAppContext) {
    let (_dirs, ws, _editor, cx) = setup(cx);
    let src = tempfile::tempdir().expect("tempdir");
    let plugin = src.path().join("notes");
    fs::create_dir_all(&plugin).expect("mkdir");
    let manifest = MANIFEST.replace("acme.tools", "acme.notes");
    fs::write(plugin.join("plugin.toml"), manifest).expect("write");
    fs::write(plugin.join("plugin.wasm"), b"not wasm").expect("write");
    let reserved = src.path().join("reserved");
    fs::create_dir_all(&reserved).expect("mkdir");
    let manifest = MANIFEST.replace("acme.tools", "arcode.tools");
    fs::write(reserved.join("plugin.toml"), manifest).expect("write");
    fs::write(reserved.join("plugin.wasm"), b"not wasm").expect("write");

    install(&ws, &reserved, cx);
    ws.read_with(cx, |ws, _| {
        let toast = ws.toast.clone().unwrap_or_default();
        assert!(toast.contains("reserved"), "{toast}");
    });
    install(&ws, &plugin, cx);
    ws.read_with(cx, |ws, _| {
        let toast = ws.toast.clone().unwrap_or_default();
        assert!(toast.contains("invalid plugin component"), "{toast}");
        assert_eq!(ws.activity, crate::activity_bar::Activity::Plugins);
    });
}

fn install(ws: &Entity<Workspace>, src: &Path, cx: &mut VisualTestContext) {
    ws.update_in(cx, |ws, window, cx| {
        ws.plugins_install_from(src.to_path_buf(), window, cx)
    });
    cx.run_until_parked();
}

#[gpui::test]
fn answers_wait_for_the_arm_delay(cx: &mut TestAppContext) {
    use crate::plugins::consent::{Choice, model};
    use crate::plugins::consent_dialog::{ARM_DELAY, ConsentDialog, Control};
    let (_ws, cx) = open(cx);
    let (ask, answer, _) = ask(network());
    let m = model(&ask.request, None);
    let dialog =
        cx.new(|cx| ConsentDialog::new(m, Some(ask.reply), ask.cancel.clone(), ARM_DELAY, cx));
    // A click that was meant for what was under the dialog does nothing.
    dialog.update(cx, |d, cx| d.press(Control::Answer(Choice::Always), cx));
    assert_eq!(dialog.read_with(cx, |d, _| d.answer()), None);
    cx.executor().advance_clock(ARM_DELAY);
    cx.run_until_parked();
    dialog.update(cx, |d, cx| d.press(Control::Answer(Choice::Deny), cx));
    assert_eq!(answer_of(answer), ConsentAnswer::Deny);
    // Answered once; a second press changes nothing.
    dialog.update(cx, |d, cx| d.press(Control::Answer(Choice::Always), cx));
    assert_eq!(
        dialog.read_with(cx, |d, _| d.answer()),
        Some(ConsentAnswer::Deny)
    );
}
