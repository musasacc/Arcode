//! Plugin wiring in the workspace: Esc cancels a running task before the
//! editor's own Escape, a lens clicked after an edit waits for fresh
//! lenses, "Plugins: Reload" retires open panels, and a plugin that gave up
//! waiting gets no scratch tab.

use std::fs;
use std::path::Path;

use arcode_buffer::Point;
use arcode_editor::EditorEvent;
use arcode_plugin_host::PluginId;
use arcode_plugin_host::bindings::guest;
use arcode_plugin_host::cancel::CancelToken;
use arcode_ui_kit::IconName;
use gpui::{Focusable, TestAppContext, VisualTestContext};

use super::tests::{open, project_dir};
use super::*;
use crate::plugins::lenses::{LENS_SET, build};
use crate::plugins::status::RunningTask;

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

[[contributes.code-lenses]]
languages = ["http"]
"#;

/// A manifest-only plugin from temp folders (no engine is created).
fn load(ws: &Entity<Workspace>, plugins: &Path, cx: &mut VisualTestContext) {
    let bundled = plugins.join("bundled");
    let dir = bundled.join("acme.tools");
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("plugin.toml"), MANIFEST).expect("write");
    fs::write(dir.join("plugin.wasm"), b"not wasm").expect("write");
    let mut paths = crate::plugins::paths::plugin_paths(
        None,
        &plugins.join("config"),
        &plugins.join("data"),
        &plugins.join("cache"),
        None,
    );
    paths.bundled = vec![bundled];
    ws.update_in(cx, |ws, window, cx| {
        assert!(ws.plugins.client.ensure_with(paths, Vec::new(), false));
        ws.plugins_loaded(window, cx);
    });
    cx.run_until_parked();
}

/// A project with `api.http`, plugins loaded and the file open.
fn http_project<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Entity<Workspace>,
    Entity<Editor>,
    &'a mut VisualTestContext,
) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let http = root.join("api.http");
    fs::write(&http, text).expect("write");
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    load(&ws, plugin_dir.path(), cx);
    ws.update_in(cx, |ws, window, cx| ws.open_file(&http, window, cx));
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&http, cx).expect("editor"));
    (dir, plugin_dir, ws, editor, cx)
}

fn running_task(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> CancelToken {
    let cancel = CancelToken::new();
    let task = RunningTask {
        plugin: PluginId::parse("acme.tools").expect("id"),
        call: 7,
        title: "Sending".into(),
        cancel: cancel.clone(),
    };
    ws.update(cx, |ws, cx| {
        ws.plugins
            .status
            .update(cx, |s, cx| s.task_started(task, cx));
        ws.plugin_install_esc(cx);
    });
    cancel
}

#[gpui::test]
fn esc_cancels_a_task_even_with_a_selection(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, editor, cx) = http_project("GET /a\n", cx);
    let select = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        editor.update_in(cx, |e, window, cx| {
            e.select_ranges(vec![Point::new(0, 0)..Point::new(0, 3)], cx);
            window.focus(&e.focus_handle(cx));
        });
    };
    let selected = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| !e.selections().primary().is_empty())
    };

    // No task: Esc is the editor's (it clears the selection).
    select(&editor, cx);
    cx.simulate_keystrokes("escape");
    assert!(!selected(&editor, cx));

    // A running task: the first Esc cancels it, the selection stays.
    let cancel = running_task(&ws, cx);
    select(&editor, cx);
    cx.simulate_keystrokes("escape");
    assert!(cancel.is_cancelled());
    assert!(selected(&editor, cx));
    // Cancelled, the task is listed until its call ends; the next Esc is
    // the editor's again.
    cx.simulate_keystrokes("escape");
    assert!(!selected(&editor, cx));

    // A consent prompt is answered by Esc, not the task.
    ws.update(cx, |ws, cx| {
        ws.plugins.status.update(cx, |s, cx| s.task_finished(7, cx))
    });
    let cancel = running_task(&ws, cx);
    ws.update(cx, |ws, _| ws.plugins.prompts_open = 1);
    cx.simulate_keystrokes("escape");
    assert!(!cancel.is_cancelled());
}

#[gpui::test]
fn a_lens_clicked_after_an_edit_waits_for_fresh_lenses(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, editor, cx) = http_project("GET /a\n\nPOST /b\n\nGET /slow\n", cx);
    let lens = |line: u32| guest::CodeLens {
        line,
        title: "Send request".into(),
        command: "send".into(),
        args: format!("{{\"line\":{line}}}"),
    };
    let plugin = PluginId::parse("acme.tools").expect("id");
    let set = build(vec![(plugin.clone(), vec![lens(4)])], 6);
    ws.update(cx, |ws, cx| {
        editor.update(cx, |e, cx| e.set_code_lenses(LENS_SET, set.lenses, cx));
        let tracked = ws.plugins.editors.get_mut(&editor.entity_id());
        tracked.expect("tracked").lens_targets = set.targets;
    });
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "\n\n".into())],
            cx,
        )
    });
    let (line, id) = editor.read_with(cx, |e, _| {
        let lens = &e.code_lenses(LENS_SET)[0];
        (lens.line, lens.items[0].id)
    });
    assert_eq!(line, 6);
    editor.update(cx, |_, cx| {
        cx.emit(EditorEvent::CodeLensClicked {
            set: LENS_SET.into(),
            line,
            id,
        })
    });
    ws.read_with(cx, |ws, _| {
        let tracked = &ws.plugins.editors[&editor.entity_id()];
        assert!(tracked.lens_targets.is_empty(), "nothing ran");
        assert_eq!(
            tracked.pending_click,
            Some((6, "Send request".into())),
            "the click waits for the lenses of the current text"
        );
    });
}

#[gpui::test]
fn reload_retires_open_panels(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, _editor, cx) = http_project("GET /a\n", cx);
    let key = (PluginId::parse("acme.tools").expect("id"), 1);
    let (view, page) = ws.update_in(cx, |ws, window, cx| {
        ws.plugin_add_page(&key, "Acme".into(), IconName::Rows, false, window, cx);
        let view = ws.plugins.panels[&key].view.upgrade().expect("view");
        (view, ws.plugins.page_key(&key))
    });
    let open = |ws: &Workspace, page: &str, cx: &gpui::App| {
        ws.panes
            .iter()
            .any(|p| p.read(cx).page_position(page).is_some())
    };
    ws.read_with(cx, |ws, cx| assert!(open(ws, &page, cx)));
    ws.update_in(cx, |ws, window, cx| ws.reload_plugins(window, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.is_retired()));
    ws.read_with(cx, |ws, cx| {
        assert!(!open(ws, &page, cx), "the dead tab is closed");
        assert!(ws.plugins.panels.is_empty());
        assert_ne!(ws.plugins.page_key(&key), page, "new panels get new tabs");
    });
}

#[gpui::test]
fn no_scratch_tab_for_a_plugin_that_gave_up(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, _editor, cx) = http_project("GET /a\n", cx);
    let editors = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editors(cx).len())
    };
    let before = editors(&ws, cx);
    let (reply, timed_out) = std::sync::mpsc::sync_channel(1);
    drop(timed_out);
    let request = crate::plugins::bridge::EditorRequest::OpenScratch {
        title: "Response".into(),
        language: "json".into(),
        text: "{}".into(),
        reply,
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_editor_request(request, window, cx)
    });
    assert_eq!(editors(&ws, cx), before);
}

/// The status bar names the plugin language of a `.http` file (the editor
/// itself only knows plain text).
#[gpui::test]
fn status_bar_names_a_plugin_language(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, _editor, cx) = http_project("GET /a\n", cx);
    ws.update(cx, |ws, cx| ws.refresh_status(cx));
    let language = ws.read_with(cx, |ws, cx| ws.status_bar.read(cx).info().language.clone());
    assert_eq!(language.as_ref().map(|l| l.as_ref()), Some("HTTP Request"));
}

/// A panel opened beside the editor gets its own split; closing it (the
/// plugin was disabled or reloaded) closes that split too, while a split
/// that still has tabs stays.
#[gpui::test]
fn closing_a_side_panel_closes_its_empty_split(cx: &mut TestAppContext) {
    let (_dir, _plugins, ws, _editor, cx) = http_project("GET /a\n", cx);
    let key = (PluginId::parse("acme.tools").expect("id"), 1);
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_add_page(&key, "Acme".into(), IconName::Rows, true, window, cx)
    });
    assert_eq!(ws.read_with(cx, |ws, _| ws.panes.len()), 2);
    ws.update_in(cx, |ws, window, cx| ws.plugin_close_panel(&key, window, cx));
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.panes.len()), 1);
}
