//! The welcome screen's list from the keyboard: a filter that never hides
//! projects once its field is gone, a selection that stays on screen, a
//! folder that came back, and Tab in the Clone Repository dialog.

use std::path::Path;

use gpui::{TestAppContext, px, size};

use super::super::tests::open;
use super::{draw, names, seed, welcome};
use crate::actions::CloneRepository;
use crate::app_state;

#[gpui::test]
fn a_hidden_filter_never_hides_projects(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let many: Vec<String> = (0..8).map(|i| format!("project-{i}")).collect();
    let mut all: Vec<&str> = many.iter().map(String::as_str).collect();
    all.push("zebra-tools");
    let (_dir, paths) = seed(&all, cx);
    draw(cx);
    let w = welcome(&ws, cx);
    cx.simulate_keystrokes("z e b");
    draw(cx);
    assert_eq!(names(&ws, cx), ["zebra-tools"]);
    assert!(cx.update(|window, cx| w.read(cx).filter_focused(window, cx)));

    // Removing the match leaves 8 projects: no field, so no filter.
    w.update(cx, |w, cx| w.remove(&paths[8], cx));
    draw(cx);
    assert!(!w.read_with(cx, |w, cx| w.shows_filter(cx)));
    assert_eq!(w.read_with(cx, |w, cx| w.filter_text(cx).to_owned()), "");
    assert_eq!(names(&ws, cx).len(), 8);
    assert!(!cx.update(|window, cx| w.read(cx).filter_focused(window, cx)));
    // The keys work on the list again.
    cx.simulate_keystrokes("down");
    let selected = w.read_with(cx, |w, cx| w.selected_path(cx));
    assert_eq!(selected.as_deref(), Some(paths[7].as_path()));
}

#[gpui::test]
fn the_welcome_screen_comes_back_unfiltered(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let many: Vec<String> = (0..10).map(|i| format!("p{i}")).collect();
    let mut all: Vec<&str> = many.iter().map(String::as_str).collect();
    all.push("weather-cli");
    let (_dir, paths) = seed(&all, cx);
    draw(cx);
    cx.simulate_keystrokes("c l i enter");
    cx.run_until_parked();
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    assert_eq!(root.as_deref(), paths.last().map(|p| p.as_path()));

    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    draw(cx);
    let w = welcome(&ws, cx);
    assert_eq!(w.read_with(cx, |w, cx| w.filter_text(cx).to_owned()), "");
    assert_eq!(names(&ws, cx).len(), 11);
}

#[gpui::test]
fn the_selected_row_is_scrolled_into_view(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.simulate_resize(size(px(1000.), px(500.)));
    let many: Vec<String> = (0..20).map(|i| format!("project-{i:02}")).collect();
    let all: Vec<&str> = many.iter().map(String::as_str).collect();
    let (_dir, _paths) = seed(&all, cx);
    draw(cx);
    let w = welcome(&ws, cx);
    assert_eq!(w.read_with(cx, |w, _| w.scroll_top()), px(0.));

    // Up starts at the last row, far below the window's bottom.
    cx.simulate_keystrokes("up");
    draw(cx);
    let down = w.read_with(cx, |w, _| w.scroll_top());
    assert!(down > px(300.), "scrolled to {down:?}");

    // Back to the first row: the screen scrolls back up.
    cx.simulate_keystrokes(&["up"; 19].join(" "));
    draw(cx);
    let first = w.read_with(cx, |w, cx| w.selected_path(cx));
    assert!(first.is_some_and(|p| p.ends_with("project-19")));
    let up = w.read_with(cx, |w, _| w.scroll_top());
    assert!(up < down, "{up:?} after {down:?}");

    // Drawing again (nothing pressed) leaves the scroll where it is.
    draw(cx);
    assert_eq!(w.read_with(cx, |w, _| w.scroll_top()), up);
}

#[gpui::test]
fn a_folder_that_came_back_opens(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (_dir, paths) = seed(&["here", "away"], cx);
    std::fs::remove_dir(&paths[1]).unwrap();
    draw(cx);
    let w = welcome(&ws, cx);
    assert!(w.read_with(cx, |w, _| w.is_missing(&paths[1])));

    std::fs::create_dir(&paths[1]).unwrap();
    w.update_in(cx, |w, window, cx| {
        w.open(paths[1].clone(), false, window, cx)
    });
    cx.run_until_parked();
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    assert_eq!(root.as_deref(), Some(paths[1].as_path()));
    assert!(!w.read_with(cx, |w, _| w.is_missing(&paths[1])));
    cx.update(|_, cx| assert_eq!(app_state::recent_projects(cx)[0].path, paths[1]));
}

#[gpui::test]
fn tab_moves_between_the_clone_fields(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.dispatch_action(CloneRepository);
    cx.run_until_parked();
    let dialog = ws
        .read_with(cx, |ws, _| ws.clone_dialog().cloned())
        .expect("the dialog is open");
    cx.simulate_keystrokes("a b");
    assert_eq!(dialog.read_with(cx, |d, cx| d.url(cx)), "ab");
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "ab");

    // Location, then the folder name, then around to the address.
    cx.simulate_keystrokes("tab tab x");
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "abx");
    assert_eq!(dialog.read_with(cx, |d, cx| d.url(cx)), "ab");
    cx.simulate_keystrokes("tab c");
    assert_eq!(dialog.read_with(cx, |d, cx| d.url(cx)), "abc");
    // The folder name was edited, so it no longer follows the address.
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "abx");
    // Shift-Tab goes back, around to the folder name.
    cx.simulate_keystrokes("shift-tab y");
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "abxy");
    assert_eq!(dialog.read_with(cx, |d, cx| d.url(cx)), "abc");
}
