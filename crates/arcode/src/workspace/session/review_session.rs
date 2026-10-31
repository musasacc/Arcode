//! Reviewer tests for session restore.

use std::fs;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_config::{ProjectSession, WindowState};
use gpui::TestAppContext;

use crate::app_state;
use crate::workspace::tests::{open, project_dir};

/// The 2 s delayed save (after a file is opened or a tab changes) calls
/// `save_session(None, ..)`, so it replaces the stored session with one
/// whose `window` is `None`. The project's saved window placement is lost
/// from `state.toml` until the next clean quit/close; after a crash or a
/// kill the project's window no longer reopens where it was.
#[gpui::test]
fn review_delayed_session_save_keeps_the_window_placement(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    let placement = WindowState {
        x: 300.,
        y: 140.,
        width: 900.,
        height: 700.,
        maximized: false,
    };
    cx.update(|_, cx| {
        app_state::set_session(
            &root,
            ProjectSession {
                window: Some(placement),
                ..Default::default()
            },
            cx,
        )
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/lib.rs"), window, cx)
    });
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let session = app_state::session(&root, cx).expect("session");
        assert_eq!(
            session.window,
            Some(placement),
            "the delayed save dropped the project's window placement"
        );
    });
}

/// Cursor and scroll of a file are only kept while its tab is open:
/// `capture_session` rebuilds `files` from the open editors, so closing a
/// tab forgets where the cursor was (`MAX_FILE_STATES` / `compact` talk
/// about keeping states of files no longer open, but none ever survive),
/// and reopening the file later, in this run or after a restart, starts
/// at the top.
#[gpui::test]
fn review_closed_file_keeps_its_cursor_in_the_session(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("src/main.rs"), "fn main() {\n    let x = 1;\n}\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
        ws.open_file(&root.join("src/lib.rs"), window, cx);
    });
    let main = ws.update(cx, |ws, cx| ws.editors(cx)[0].clone());
    main.update(cx, |e, cx| {
        let at = e.point_to_offset(Point::new(1, 8));
        e.set_selections(
            arcode_buffer::SelectionSet::from_vec(vec![arcode_buffer::Selection::cursor(at)], 0)
                .expect("selection"),
            cx,
        );
    });
    // Close main.rs (clean, so no prompt).
    ws.update_in(cx, |ws, window, cx| {
        let pane = ws.active_pane().clone();
        pane.update(cx, |pane, cx| pane.close(0, window, cx));
    });
    cx.run_until_parked();
    let session = ws
        .update(cx, |ws, cx| ws.capture_session(None, cx))
        .expect("session");
    assert!(
        session.file(&root.join("src/main.rs")).is_some_and(|f| f
            .selections
            .first()
            .map(|s| s.head)
            == Some((1, 8))),
        "main.rs's cursor was forgotten when its tab closed: {:?}",
        session.files.keys().collect::<Vec<_>>()
    );
}
