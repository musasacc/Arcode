//! Plugin runtime wiring: lens clicks after an edit, plugin tabs after
//! "Plugins: Reload" and typing into panel inputs. Each test pins a bug
//! that was fixed.

use std::fs;
use std::path::Path;

use arcode_buffer::Point;
use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::guest;
use arcode_plugin_host::bindings::ui::{
    Axis, ButtonProps, InputProps, Node, StackProps, View, Widget,
};
use arcode_plugin_host::ui_tree::PanelTree;
use arcode_plugin_host::{PluginId, QualifiedId};
use arcode_ui_kit::IconName;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};

use super::tests::{open, project_dir};
use super::*;
use crate::activity_bar::Activity;
use crate::plugins::lenses::{LENS_SET, build};
use crate::plugins::{PanelView, Placement};

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
id = "send"
title = "Acme: Send"

[[contributes.code-lenses]]
languages = ["http"]

[[contributes.panels]]
id = "view"
title = "Acme View"
location = "editor"

[[contributes.panels]]
id = "side"
title = "Acme Side"
location = "sidebar"
icon = "info"
"#;

/// Loads a manifest-only plugin (no engine is ever created) from temp
/// folders, never from the user's config.
fn load_plugins(ws: &Entity<Workspace>, plugins: &Path, cx: &mut VisualTestContext) {
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

/// The lenses come back for a version; the user inserts two lines at the
/// top; within the 300 ms debounce (plus the plugin's answer) the lens row
/// has already moved down with its line, but its click target still holds
/// the plugin's args for the *old* line. Clicking "Send request" above
/// `GET /slow` then sends whatever request is now at the old line (seen
/// in the app: a POST /echo was sent).
#[gpui::test]
fn lens_click_after_edit_uses_stale_target(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let http = root.join("api.http");
    fs::write(&http, "GET /a\n\nPOST /b\n\nGET /slow\n").expect("write");
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    load_plugins(&ws, plugin_dir.path(), cx);
    ws.update_in(cx, |ws, window, cx| ws.open_file(&http, window, cx));
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&http, cx).expect("editor"));

    // The plugin's answer for the current version: a lens above line 4.
    let plugin = PluginId::parse("acme.tools").expect("id");
    let answer = guest::CodeLens {
        line: 4,
        title: "Send request".into(),
        command: "send".into(),
        args: "{\"line\":4}".into(),
    };
    ws.update(cx, |ws, cx| {
        let lines = editor.read(cx).line_count();
        let set = build(vec![(plugin, vec![answer])], lines);
        editor.update(cx, |e, cx| e.set_code_lenses(LENS_SET, set.lenses, cx));
        let tracked = ws
            .plugins
            .editors
            .get_mut(&editor.entity_id())
            .expect("tracked");
        tracked.lens_targets = set.targets;
    });

    // Two lines typed at the top.
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "\n\n".into())],
            cx,
        )
    });
    cx.run_until_parked();

    ws.read_with(cx, |ws, cx| {
        let lens = editor.read(cx).code_lenses(LENS_SET)[0].clone();
        assert_eq!(lens.line, 6, "the lens row moved with its line");
        let target = ws.plugins.editors[&editor.entity_id()]
            .lens_targets
            .get(&lens.items[0].id)
            .cloned();
        if let Some(target) = target {
            assert_eq!(
                target.args,
                format!("{{\"line\":{}}}", lens.line),
                "a click on the lens above line {} would run the request at the old line",
                lens.line
            );
        }
    });
}

/// "Plugins: Reload" stops every instance but leaves their tabs open and
/// their slots in `plugins.panels`. The restarted instance numbers panels
/// from 1 again, so the next panel it opens (another `.db` file) has the
/// same key as the dead tab: the workspace activates the dead tab instead
/// of showing the new panel (seen in the app: "UiError::NoSuchPanel").
#[gpui::test]
fn reload_leaves_dead_tabs_that_shadow_new_panels(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    load_plugins(&ws, plugin_dir.path(), cx);

    let plugin = PluginId::parse("acme.tools").expect("id");
    let key = (plugin, 1);
    let before = ws.update_in(cx, |ws, window, cx| {
        ws.plugin_add_page(
            &key,
            "shop.sqlite".into(),
            IconName::Rows,
            false,
            window,
            cx,
        );
        ws.plugins.panels[&key].view.upgrade().expect("view")
    });
    ws.update_in(cx, |ws, window, cx| ws.reload_plugins(window, cx));
    cx.run_until_parked();

    // The restarted instance opens its first panel: id 1 again.
    let contribution = QualifiedId::parse("acme.tools/view").expect("id");
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_panel_opened(
            key.clone(),
            &contribution,
            "other.db".into(),
            None,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let after = ws.read_with(cx, |ws, _| ws.plugins.panels[&key].view.upgrade());
    assert!(
        after.is_some_and(|v| v.entity_id() != before.entity_id()),
        "the new panel must get its own view, not the tab of the instance Reload stopped"
    );
}

/// `apply-edits` waits at most 1 s for the UI thread and then tells the
/// plugin `denied`, but the request stays queued: when the UI gets to it
/// later it still applies the edit (the reply's receiver is gone). The
/// plugin believes nothing changed while the document did.
#[gpui::test]
fn apply_edits_after_the_plugin_gave_up(cx: &mut TestAppContext) {
    use arcode_plugin_host::bindings::editor::TextEdit;
    use arcode_plugin_host::bindings::{Position, Range};

    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let http = root.join("api.http");
    fs::write(&http, "GET /a\n").expect("write");
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    load_plugins(&ws, plugin_dir.path(), cx);
    ws.update_in(cx, |ws, window, cx| ws.open_file(&http, window, cx));
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&http, cx).expect("editor"));
    let version = editor.read_with(cx, |e, _| e.buffer().version());

    // The plugin thread timed out: its receiver is dropped.
    let (reply, timed_out) = std::sync::mpsc::sync_channel(1);
    drop(timed_out);
    let at = Position { line: 0, column: 0 };
    let request = crate::plugins::bridge::EditorRequest::ApplyEdits {
        doc: editor.entity_id().as_u64(),
        expected: version,
        edits: vec![TextEdit {
            range: Range { start: at, end: at },
            text: "# late\n".into(),
        }],
        reply,
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_editor_request(request, window, cx)
    });
    let text = editor.read_with(cx, |e, _| e.buffer().text().to_string());
    assert_eq!(
        text, "GET /a\n",
        "an edit the plugin was told was denied must not be applied"
    );
}

/// A row like the example plugin's: an input that grows, then a button.
/// `input` is the input's node id.
fn name_row(input: NodeId) -> View {
    let stack = |id, axis, children| {
        let props = StackProps {
            axis,
            gap: 2,
            padding: 3,
        };
        Node {
            id,
            widget: Widget::Stack(props),
            children,
            grow: false,
        }
    };
    let name = Node {
        id: input,
        widget: Widget::Input(InputProps {
            value: String::new(),
            placeholder: "Your name".into(),
            multiline: false,
            language: None,
            submit_on_enter: true,
            notify_changes: false,
        }),
        children: vec![],
        grow: true,
    };
    let greet = Node {
        id: input + 1,
        widget: Widget::Button(ButtonProps {
            label: "Greet".into(),
            icon: None,
            primary: true,
            enabled: true,
        }),
        children: vec![],
        grow: false,
    };
    View {
        root: 1,
        nodes: vec![
            stack(1, Axis::Vertical, vec![2]),
            stack(2, Axis::Horizontal, vec![input, input + 1]),
            name,
            greet,
        ],
    }
}

/// Shows [`name_row`] in `view`, clicks input `id` (drawn as `selector`)
/// and types `text`; returns what the input holds afterwards.
fn type_into(
    view: &Entity<PanelView>,
    (id, selector): (NodeId, &'static str),
    text: &str,
    cx: &mut VisualTestContext,
) -> Option<String> {
    let (snapshot, changes) = PanelTree::new().render(name_row(id)).expect("valid tree");
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    cx.run_until_parked();
    let input = cx.debug_bounds(selector).expect("the input is drawn");
    // Off the middle line: a 0 px tall field there still counts as hit.
    let at = point(input.center().x, input.top() + px(8.));
    cx.simulate_click(at, Modifiers::none());
    cx.simulate_input(text);
    view.read_with(cx, |v, cx| v.input_text(id, cx))
}

/// A plugin panel's single-line input took no clicks or text: the field
/// was drawn 0 px tall (so no placeholder either), clicks fell through to
/// the panel, which took focus for itself, and in an editor pane without
/// editor tabs the pane took it as well. Typing must reach the field both
/// in a page tab and in the sidebar.
#[gpui::test]
fn panel_inputs_take_typed_text(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    load_plugins(&ws, plugin_dir.path(), cx);
    let plugin = PluginId::parse("acme.tools").expect("id");

    // A page tab in a pane with no editor tabs.
    let page_key = (plugin.clone(), 1);
    let page = ws.update_in(cx, |ws, window, cx| {
        assert!(ws.editors(cx).is_empty());
        ws.plugin_add_page(&page_key, "Acme".into(), IconName::Rows, false, window, cx);
        ws.plugins.panels[&page_key].view.upgrade().expect("view")
    });
    assert_eq!(
        type_into(&page, (3, "plugin-input-3"), "Ada", cx).as_deref(),
        Some("Ada")
    );

    // The sidebar panel.
    let side_key = (plugin, 2);
    let side = ws.update_in(cx, |ws, _, cx| {
        let entry = ws.plugins.contributions.sidebar[0].clone();
        let view = ws.plugin_new_view(&side_key, Placement::Sidebar(entry.id.clone()), cx);
        ws.plugins.sidebar_views.insert(entry.id, view.clone());
        ws.plugins.sidebar_active = Some(0);
        ws.activity = Activity::Extension;
        ws.sidebar_visible = true;
        cx.notify();
        view
    });
    assert_eq!(
        type_into(&side, (13, "plugin-input-13"), "Grace", cx).as_deref(),
        Some("Grace")
    );
}
