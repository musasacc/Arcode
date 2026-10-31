//! Review tests for inlay hints, code actions, rename and formatting.
//! Each test reproduces a bug found in review; they fail until it is fixed.

mod common;

use std::time::Duration;

use common::lang_fake::*;
use common::*;
use gpui::TestAppContext;

/// Format Document (or Format Selection) pressed while a format-on-save is
/// waiting for a slow formatter replaces `lang.format.task`, which drops the
/// pending save task: the file is never written and `saving` stays `true`
/// forever, so every later Ctrl+S in that editor is ignored.
#[gpui::test]
fn format_while_a_formatted_save_waits_still_saves(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "").unwrap();
    let mut config = config();
    config.editor.format_on_save = true;
    config.editor.format_on_save_timeout_ms = 200;
    let (editor, cx) = open_at(cx, CODE, path.to_str().unwrap(), config);
    install_services(&editor, true, cx);

    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_saving()));
    // The user formats by hand while the save waits.
    cx.simulate_keystrokes("shift-alt-f");
    cx.run_until_parked();

    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        CODE,
        "the save that was pending must still happen after its timeout"
    );
    assert!(
        !editor.read_with(cx, |e, _| e.is_saving()),
        "the editor must not stay in the saving state"
    );

    // And a later Ctrl+S must not be ignored.
    cx.simulate_input("x");
    cx.simulate_keystrokes("secondary-s");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert!(
        std::fs::read_to_string(&path).unwrap().ends_with('x'),
        "Ctrl+S after the interrupted save must save"
    );
}
