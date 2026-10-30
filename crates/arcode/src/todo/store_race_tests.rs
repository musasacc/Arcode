//! The race between a full scan and incremental updates.

use std::fs;

use gpui::{AppContext, TestAppContext};

use super::*;
use crate::test_support::{init, settle};

/// A file saved while a full scan runs: the scan may have read it before
/// the save. Its stale result must not overwrite the newer items; the
/// file is scanned again once the full scan is in place.
#[gpui::test]
fn a_change_during_a_full_scan_survives_the_scan_result(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let file = root.join("a.rs");
    fs::write(&file, "fn a() {}\n").expect("write");
    let store = cx.new(TodoStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.clone()), cx));
    settle(cx);
    assert_eq!(store.read_with(cx, |s, _| s.count()), 0);

    // A full scan starts (and, say, reads a.rs before the save below).
    store.update(cx, |s, cx| {
        s.rescan(cx);
        // Stand in for the real scan task, whose timing a test cannot
        // control; its stale result is applied by hand below.
        s.scan_task = None;
    });
    fs::write(&file, "// TODO: new\n").expect("write");
    store.update(cx, |s, cx| {
        s.paths_changed(std::slice::from_ref(&file), false, cx)
    });
    settle(cx);
    // The full scan ends with what it read before the save.
    store.update(cx, |s, cx| s.apply_full_scan(Vec::new(), cx));
    settle(cx);
    let texts: Vec<String> = store.read_with(cx, |s, _| {
        s.files()
            .values()
            .flatten()
            .map(|i| i.text.clone())
            .collect()
    });
    assert_eq!(texts, ["new"], "the saved TODO is listed after the scan");
}
