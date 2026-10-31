//! Workspace navigation without a language server: outline symbols, the
//! outline definition fallback, back / forward across files and
//! read-only targets outside the project.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcode_buffer::Point;
use arcode_config::{Config, ThemeRegistry};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

use super::item::{NavPos, NavTarget};
use crate::workspace::Workspace;

fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        cx.set_global(crate::theme_sync::Themes(registry));
        crate::app_state::init_in_memory(cx);
        arcode_editor::init(cx);
        crate::actions::bind_keys(cx);
        crate::text_field::bind_keys(cx);
        crate::finder::bind_keys(cx);
    });
    cx.add_window_view(|window, cx| {
        let ws = Workspace::new(window, cx);
        ws.lsp().update(cx, |lsp, _| lsp.set_enabled(false));
        window.focus(&ws.focus_handle(cx));
        ws
    })
}

/// Go Back / Go Forward as bound on this platform (on macOS Alt+Left
/// moves the cursor by a word; Ctrl+- and Ctrl+Shift+- navigate).
const BACK: &str = if cfg!(target_os = "macos") {
    "ctrl--"
} else {
    "alt-left"
};
const FORWARD: &str = if cfg!(target_os = "macos") {
    "ctrl-_"
} else {
    "alt-right"
};

const MAIN: &str = "fn main() {\n    helper();\n}\n\nfn helper() {}\n\nstruct Point;\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("Cargo.toml"), "[package]\n").expect("write");
    fs::write(root.join("src/main.rs"), MAIN).expect("write");
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("write");
    (dir, root)
}

fn open_project(ws: &Entity<Workspace>, root: &Path, file: &str, cx: &mut VisualTestContext) {
    let root = root.to_path_buf();
    let file = root.join(file);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&file, window, cx);
    });
    cx.run_until_parked();
}

fn active(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> (String, Point) {
    ws.read_with(cx, |ws, cx| {
        let e = ws.active_editor(cx).expect("editor");
        let e = e.read(cx);
        let name = e
            .path()
            .and_then(|p| p.file_name())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (name, e.cursor_position())
    })
}

#[gpui::test]
fn document_symbols_fall_back_to_the_outline(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (ws, cx) = open(cx);
    open_project(&ws, &root, "src/main.rs", cx);
    cx.simulate_keystrokes("secondary-shift-o");
    cx.run_until_parked();
    let picker = ws
        .read_with(cx, |ws, _| ws.nav.picker().cloned())
        .expect("picker");
    let labels = picker.read_with(cx, |p, _| p.visible_labels());
    assert_eq!(labels, vec!["main", "helper", "Point"]);
    cx.simulate_input("poi");
    cx.run_until_parked();
    assert_eq!(
        picker.read_with(cx, |p, _| p.visible_labels()),
        vec!["Point"]
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.nav.picker().is_none()));
    assert_eq!(active(&ws, cx), ("main.rs".into(), Point::new(6, 7)));
    // The jump is in the history.
    cx.simulate_keystrokes(BACK);
    cx.run_until_parked();
    assert_eq!(active(&ws, cx).1, Point::new(0, 0));
    cx.simulate_keystrokes(FORWARD);
    cx.run_until_parked();
    assert_eq!(active(&ws, cx).1, Point::new(6, 7));
}

#[gpui::test]
fn definition_without_a_server_uses_the_outline(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (ws, cx) = open(cx);
    open_project(&ws, &root, "src/main.rs", cx);
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx)).unwrap();
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(1, 6)..Point::new(1, 6)], cx)
    });
    cx.simulate_keystrokes("f12");
    cx.run_until_parked();
    assert_eq!(active(&ws, cx), ("main.rs".into(), Point::new(4, 3)));
}

#[gpui::test]
fn back_and_forward_follow_file_switches(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (ws, cx) = open(cx);
    open_project(&ws, &root, "src/main.rs", cx);
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx)).unwrap();
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(4, 2)..Point::new(4, 2)], cx)
    });
    let lib = root.join("src/lib.rs");
    ws.update_in(cx, |ws, window, cx| ws.open_file(&lib, window, cx));
    cx.run_until_parked();
    assert_eq!(active(&ws, cx).0, "lib.rs");
    cx.simulate_keystrokes(BACK);
    cx.run_until_parked();
    assert_eq!(active(&ws, cx), ("main.rs".into(), Point::new(4, 2)));
    cx.simulate_keystrokes(FORWARD);
    cx.run_until_parked();
    assert_eq!(active(&ws, cx).0, "lib.rs");
    // Going back again does not record the switch as a new jump.
    cx.simulate_keystrokes(BACK);
    cx.run_until_parked();
    assert_eq!(active(&ws, cx).0, "main.rs");
    assert!(ws.read_with(cx, |ws, _| ws.nav.history().can_go_forward()));
}

#[gpui::test]
fn targets_outside_the_project_open_read_only(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let outside_dir = tempfile::tempdir().expect("tempdir");
    let outside = fs::canonicalize(outside_dir.path())
        .unwrap()
        .join("option.rs");
    fs::write(
        &outside,
        "pub enum Option<T> {\n    None,\n    Some(T),\n}\n",
    )
    .unwrap();
    let (ws, cx) = open(cx);
    open_project(&ws, &root, "src/main.rs", cx);
    let target = NavTarget {
        path: outside.clone(),
        pos: NavPos::Points(Point::new(2, 4)..Point::new(2, 8)),
    };
    ws.update_in(cx, |ws, window, cx| ws.jump_to(&target, window, cx));
    cx.run_until_parked();
    let (read_only, at) = ws.read_with(cx, |ws, cx| {
        let e = ws.active_editor(cx).unwrap();
        let e = e.read(cx);
        (e.is_read_only(), e.cursor_position())
    });
    assert!(read_only);
    assert_eq!(at, Point::new(2, 4));
    // Project files stay editable.
    let main = root.join("src/main.rs");
    let target = NavTarget {
        path: main,
        pos: NavPos::Points(Point::new(0, 3)..Point::new(0, 7)),
    };
    ws.update_in(cx, |ws, window, cx| ws.jump_to(&target, window, cx));
    cx.run_until_parked();
    assert!(!ws.read_with(cx, |ws, cx| {
        ws.active_editor(cx).unwrap().read(cx).is_read_only()
    }));
}

#[gpui::test]
fn workspace_symbols_without_a_server_list_open_outlines(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (ws, cx) = open(cx);
    open_project(&ws, &root, "src/main.rs", cx);
    cx.simulate_keystrokes("secondary-t");
    cx.run_until_parked();
    let picker = ws
        .read_with(cx, |ws, _| ws.nav.picker().cloned())
        .expect("picker");
    let labels = picker.read_with(cx, |p, _| p.visible_labels());
    assert_eq!(labels, vec!["main", "helper", "Point"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.nav.picker().is_none()));
}
