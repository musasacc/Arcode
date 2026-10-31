use std::fs;

use arcode_buffer::{Point, Selection, SelectionSet};
use arcode_config::{PaneSession, ProjectSession};
use gpui::{TestAppContext, px};

use super::*;
use crate::workspace::tests::{open, project_dir};

fn select(editor: &gpui::Entity<arcode_editor::Editor>, cx: &mut gpui::VisualTestContext) {
    editor.update(cx, |e, cx| {
        let a = e.point_to_offset(Point::new(0, 3));
        let b = e.point_to_offset(Point::new(0, 7));
        let c = e.point_to_offset(Point::new(1, 2));
        let set = SelectionSet::from_vec(vec![Selection::new(a, b), Selection::cursor(c)], 0)
            .expect("selections");
        e.set_selections(set, cx);
    });
}

#[gpui::test]
fn session_round_trips_through_close_and_reopen(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("src/main.rs"), "fn main() {\n    let x = 1;\n}\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/main.rs"), window, cx);
        ws.open_file(&root.join("Cargo.toml"), window, cx);
        ws.split_right(window, cx);
        ws.open_file(&root.join("src/lib.rs"), window, cx);
        ws.sidebar_width = px(300.);
        ws.activity = Activity::Git;
        ws.bottom_panel_visible = true;
        ws.bottom_panel_height = px(200.);
        ws.bottom_panel
            .update(cx, |p, cx| p.set_active_tab(BottomTab::Problems, cx));
        // Back to the first pane, on main.rs.
        ws.active_pane = 0;
        let pane = ws.panes[0].clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    let main = ws.update(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    select(&main, cx);
    if let Some(tree) = ws.read_with(cx, |ws, _| ws.file_tree.clone()) {
        tree.update(cx, |t, cx| t.set_expanded_dirs(&[root.join("src")], cx));
    }

    let session = ws
        .update(cx, |ws, cx| ws.capture_session(None, cx))
        .expect("session");
    assert_eq!(session.panes.len(), 2);
    assert_eq!(
        session.panes[0].files,
        [root.join("src/main.rs"), root.join("Cargo.toml")]
    );
    assert_eq!(session.panes[0].active, 0);
    assert_eq!(session.panes[1].files, [root.join("src/lib.rs")]);
    assert_eq!(session.active_pane, 0);
    let main_state = &session.files[&ProjectSession::file_key(&root.join("src/main.rs"))];
    assert_eq!(main_state.selections.len(), 2);
    assert_eq!(main_state.selections[0].anchor, (0, 3));
    assert_eq!(main_state.selections[0].head, (0, 7));
    assert_eq!(main_state.selections[1].head, (1, 2));
    let layout = session.layout.clone().expect("layout");
    assert_eq!(layout.sidebar_view, "git");
    assert_eq!(layout.sidebar_width, 300.);
    assert!(layout.panel_visible);
    assert_eq!(layout.panel_tab, "problems");
    assert_eq!(session.expanded_dirs, [root.join("src")]);

    // Close (saves the session), then open the project again.
    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert!(ws.shows_welcome(cx));
        assert!(app_state::session(&root, cx).is_some());
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.panes.len(), 2);
        let titles = |ix: usize| -> Vec<String> {
            ws.panes[ix]
                .read(cx)
                .items()
                .iter()
                .map(|e| e.read(cx).title().to_string())
                .collect()
        };
        assert_eq!(titles(0), ["main.rs", "Cargo.toml"]);
        assert_eq!(titles(1), ["lib.rs"]);
        assert_eq!(ws.active_pane, 0);
        let active = ws.active_editor(cx).expect("active");
        let e = active.read(cx);
        assert_eq!(e.title().to_string(), "main.rs");
        assert_eq!(e.selections().len(), 2);
        assert_eq!(e.cursor_position(), Point::new(0, 7));
        assert_eq!(ws.activity, Activity::Git);
        assert_eq!(ws.sidebar_width, px(300.));
        assert!(ws.bottom_panel_visible);
        assert_eq!(ws.bottom_panel.read(cx).active_tab(), BottomTab::Problems);
        let tree = ws.file_tree.as_ref().expect("tree").read(cx);
        assert_eq!(tree.expanded_dirs(), [root.join("src")]);
    });
}

#[gpui::test]
fn missing_files_are_skipped_and_restore_can_be_turned_off(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    let session = ProjectSession {
        panes: vec![
            PaneSession {
                files: vec![root.join("gone.rs")],
                active: 0,
            },
            PaneSession {
                files: vec![root.join("src/lib.rs"), root.join("also-gone.rs")],
                active: 1,
            },
        ],
        active_pane: 1,
        ..Default::default()
    };
    cx.update(|_, cx| app_state::set_session(&root, session.clone(), cx));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update(cx, |ws, cx| {
        assert_eq!(
            ws.panes.len(),
            1,
            "the pane with only missing files is dropped"
        );
        assert_eq!(ws.editors(cx).len(), 1);
        assert_eq!(
            ws.active_editor(cx).map(|e| e.read(cx).title().to_string()),
            Some("lib.rs".into())
        );
    });

    // With restore_session off a project opens empty.
    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        app_state::set_session(&root, session, cx);
        let mut config = arcode_ui_kit::settings(cx).clone();
        config.workspace.restore_session = false;
        arcode_ui_kit::set_settings(config, cx);
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update(cx, |ws, cx| assert!(ws.editors(cx).is_empty()));
}

#[gpui::test]
fn sidebar_and_panel_names_round_trip(_: &mut TestAppContext) {
    for activity in Activity::ALL {
        assert_eq!(activity_from_key(activity_key(activity)), Some(activity));
    }
    for tab in BottomTab::ALL {
        assert_eq!(tab_from_key(tab_key(tab)), Some(tab));
    }
    assert_eq!(activity_from_key("nope"), None);
}

#[gpui::test]
fn a_reopened_file_goes_back_to_its_cursor(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let main_rs = root.join("src/main.rs");
    fs::write(&main_rs, "fn main() {\n    let x = 1;\n}\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    let main = ws.update(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    main.update(cx, |e, cx| {
        let at = e.point_to_offset(Point::new(1, 8));
        e.set_selections(
            SelectionSet::from_vec(vec![Selection::cursor(at)], 0).expect("selection"),
            cx,
        );
    });
    ws.update_in(cx, |ws, window, cx| {
        let pane = ws.active_pane().clone();
        pane.update(cx, |pane, cx| pane.close(0, window, cx));
    });
    // The close event is handled after that update; then reopen.
    ws.update_in(cx, |ws, window, cx| {
        assert!(ws.editors(cx).is_empty());
        ws.open_file(&main_rs, window, cx);
    });
    ws.update(cx, |ws, cx| {
        let editor = ws.active_editor(cx).expect("reopened");
        assert_eq!(editor.read(cx).cursor_position(), Point::new(1, 8));
    });

    // A deleted file's state is forgotten.
    ws.update_in(cx, |ws, window, cx| {
        let pane = ws.active_pane().clone();
        pane.update(cx, |pane, cx| pane.close(0, window, cx));
    });
    ws.update_in(cx, |ws, window, cx| {
        assert!(
            ws.capture_session(None, cx)
                .expect("session")
                .file(&main_rs)
                .is_some()
        );
        fs::remove_file(&main_rs).expect("rm");
        ws.on_tree_event(
            &crate::sidebar::file_tree::FileTreeEvent::Deleted(main_rs.clone()),
            window,
            cx,
        );
    });
    let session = ws
        .update(cx, |ws, cx| ws.capture_session(None, cx))
        .expect("session");
    assert!(session.file(&main_rs).is_none());
}
