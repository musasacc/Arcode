//! Review tests for the Search view (failing tests that reproduce bugs
//! found in review).

use std::fs;

use gpui::TestAppContext;

use super::*;
use crate::test_support::init;

/// Replace All counts the matches in the background and then asks "Replace
/// N occurrences in M files with “omega”?". The replace that runs after the
/// confirmation rebuilds the query and replacement from the *current*
/// fields (`replace_files` -> `replace_query`), so anything typed while the
/// count ran (which takes seconds on a big project) is what gets written,
/// not what the dialog showed and the user confirmed.
#[gpui::test]
fn replace_all_writes_what_the_confirmation_showed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("a.txt"), "alpha\n").expect("write");
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ProjectSearch::new(cx));
    view.update(cx, |v, cx| {
        v.set_root(Some(root.clone()), cx);
        v.set_query("alpha", cx);
        v.set_replacement("omega", cx);
        v.run(true, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |v, window, cx| v.replace_all(window, cx));
    // The user keeps typing in the replace field while the count runs.
    view.update(cx, |v, cx| v.set_replacement("omegaX", cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "confirmation shown");
    cx.simulate_prompt_answer("Replace All");
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(root.join("a.txt")).unwrap(),
        "omega\n",
        "the confirmed replacement (“omega”) must be written"
    );
}

/// Enter in the Replace field (or in a glob field) used to run `Confirm`
/// of the whole view: it opened the selected or first match and moved
/// focus to the editor. It now searches again and stays in the field;
/// Ctrl+Alt+Enter there asks to Replace All.
#[gpui::test]
fn enter_in_the_replace_field_stays_in_the_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("a.txt"), "alpha\n").expect("write");
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ProjectSearch::new(cx));
    let opened = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen = opened.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ProjectSearchEvent, _| {
            if matches!(event, ProjectSearchEvent::Open { .. }) {
                seen.set(seen.get() + 1);
            }
        })
        .detach();
    });
    view.update_in(cx, |v, window, cx| {
        v.set_root(Some(root.clone()), cx);
        v.focus_query(Some("alpha".into()), true, window, cx);
        v.run(true, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |v, window, cx| {
        v.set_replacement("omega", cx);
        window.focus(&v.replacement.focus_handle(cx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(opened.get(), 0, "Enter in the replace field opens nothing");
    let focused = cx.update(|window, cx| {
        view.read(cx)
            .replacement
            .focus_handle(cx)
            .is_focused(window)
    });
    assert!(focused, "focus stays in the replace field");

    cx.simulate_keystrokes("secondary-alt-enter");
    cx.run_until_parked();
    assert!(
        cx.has_pending_prompt(),
        "Ctrl+Alt+Enter (Cmd+Alt+Enter on macOS) asks to replace all"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "alpha\n");
}
