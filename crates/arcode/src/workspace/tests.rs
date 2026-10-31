use std::fs;
use std::sync::Arc;

use arcode_config::{Config, ThemeRegistry};
use gpui::{TestAppContext, VisualTestContext};

use super::*;
use crate::actions::{SplitRight, ToggleFinder};
use crate::app_state;

pub(super) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        cx.set_global(crate::theme_sync::Themes(registry));
        app_state::init_in_memory(cx);
        arcode_editor::init(cx);
        crate::actions::bind_keys(cx);
        crate::text_field::bind_keys(cx);
        crate::finder::bind_keys(cx);
        crate::find_bar::bind_keys(cx);
        crate::search_view::bind_keys(cx);
        crate::commands::init(cx);
    });
}

pub(super) fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("Cargo.toml"), "[package]\n").expect("write");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write");
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("write");
    dir
}

pub(super) fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    init(cx);
    cx.add_window_view(|window, cx| {
        let ws = Workspace::new(window, cx);
        // Never start real language servers in tests.
        ws.lsp.update(cx, |lsp, _| lsp.set_enabled(false));
        window.focus(&ws.focus_handle);
        ws
    })
}

#[gpui::test]
fn project_files_panes_and_status(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);

    ws.update(cx, |ws, cx| assert!(ws.shows_welcome(cx)));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert!(!ws.shows_welcome(cx));
        assert_eq!(
            ws.project().map(|p| p.name.clone()),
            Some(project_name(&root))
        );
        assert_eq!(ws.project_files.len(), 3);
        assert_eq!(app_state::recent_projects(cx)[0].path, root);
        assert_eq!(
            ws.project().and_then(|p| p.run_command()),
            Some("cargo run")
        );
    });

    let main_rs = root.join("src/main.rs");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&main_rs, window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.editors(cx).len(), 1, "reopening activates the open tab");
        let info = ws.status_bar().read(cx).info().clone();
        assert_eq!(info.language.as_ref().map(|l| l.as_ref()), Some("Rust"));
        assert_eq!(info.cursor, Some((0, 0)));
    });

    cx.dispatch_action(SplitRight);
    let lib_rs = root.join("src/lib.rs");
    ws.update_in(cx, |ws, window, cx| ws.open_file(&lib_rs, window, cx));
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.panes().len(), 2);
        assert_eq!(ws.active_pane, 1);
        assert_eq!(ws.panes()[1].read(cx).items().len(), 1);
    });
    // Opening a file already shown in the left pane switches back to it.
    ws.update_in(cx, |ws, window, cx| ws.open_file(&main_rs, window, cx));
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.active_pane, 0);
        assert_eq!(ws.editors(cx).len(), 2);
    });

    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    ws.update(cx, |ws, cx| {
        assert!(ws.shows_welcome(cx));
        assert_eq!(ws.panes().len(), 1);
    });
}

#[gpui::test]
fn finder_opens_files_by_fuzzy_query(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();

    cx.dispatch_action(ToggleFinder);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.finder.is_some()));
    cx.simulate_input("lib");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert!(ws.finder.is_none());
        let editor = ws.active_editor(cx).expect("an editor");
        assert_eq!(
            editor.read(cx).path(),
            Some(root.join("src/lib.rs").as_path())
        );
    });

    cx.dispatch_action(ToggleFinder);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.finder.is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.finder.is_none()));
}

fn project_name(path: &Path) -> String {
    arcode_config::state::project_name(path)
}

#[gpui::test]
fn tree_selection_follows_the_active_tab(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    let main_rs = root.join("src/main.rs");
    let lib_rs = root.join("src/lib.rs");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&main_rs, window, cx);
        ws.open_file(&lib_rs, window, cx);
    });
    let selected = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let tree = ws.file_tree.as_ref().expect("tree");
            tree.read(cx).selected_path().map(Path::to_path_buf)
        })
    };
    assert_eq!(selected(&ws, cx), Some(lib_rs.clone()));
    let pane = ws.read_with(cx, |ws, _| ws.panes()[0].clone());
    pane.update_in(cx, |pane, window, cx| pane.close(1, window, cx));
    cx.run_until_parked();
    assert_eq!(selected(&ws, cx), Some(main_rs.clone()));
    pane.update_in(cx, |pane, window, cx| pane.close(0, window, cx));
    cx.run_until_parked();
    assert_eq!(
        selected(&ws, cx),
        None,
        "closing the last tab clears the tree selection"
    );
}
