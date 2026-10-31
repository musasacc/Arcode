//! Workspace edits with resource operations, dirty files, focus and the
//! summary.

use std::fs;
use std::path::{Path, PathBuf};

use arcode_lsp::lsp_types::{Position, Range, TextEdit};
use arcode_lsp::{EditReply, PositionEncoding};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

use super::Summary;
use crate::lsp_bridge::edits::{EditRequest, EditSource};
use crate::lsp_bridge::workspace_edit::FileOp;
use crate::workspace::Workspace;

fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    crate::test_support::init(cx);
    cx.add_window_view(|window, cx| {
        let ws = Workspace::new(window, cx);
        ws.lsp().update(cx, |lsp, _| lsp.set_enabled(false));
        window.focus(&ws.focus_handle(cx));
        ws
    })
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/main.rs"), "mod util;\nfn main() {}\n").expect("write");
    fs::write(root.join("src/util.rs"), "pub fn util() {}\n").expect("write");
    fs::write(root.join("src/other.rs"), "// other\n").expect("write");
    (dir, root)
}

fn edit_op(path: &Path, line: u32, from: u32, to: u32, text: &str) -> FileOp {
    FileOp::Edit {
        path: path.to_path_buf(),
        version: None,
        edits: vec![TextEdit::new(
            Range::new(Position::new(line, from), Position::new(line, to)),
            text.into(),
        )],
    }
}

fn run(
    ws: &Entity<Workspace>,
    ops: Vec<FileOp>,
    cx: &mut VisualTestContext,
) -> Result<Summary, String> {
    let (reply, _rx) = EditReply::channel();
    let request = EditRequest {
        ops,
        encoding: PositionEncoding::Utf16,
        label: Some("Rename".into()),
        reply,
        source: EditSource::Editor,
    };
    let result = ws.update_in(cx, |ws, window, cx| {
        ws.apply_edit_request(&request, window, cx)
    });
    cx.run_until_parked();
    result
}

fn editor(
    ws: &Entity<Workspace>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<arcode_editor::Editor> {
    ws.read_with(cx, |ws, cx| {
        ws.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(path))
            .expect("open editor")
    })
}

#[gpui::test]
fn resource_operations_save_the_text_that_goes_with_them(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let other = root.join("src/other.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&other, window, cx);
    });
    cx.run_until_parked();
    // `other.rs` has unsaved changes of the user's.
    let other_editor = editor(&ws, &other, cx);
    other_editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(
                arcode_buffer::Point::new(0, 0)..arcode_buffer::Point::new(0, 0),
                "x".into(),
            )],
            cx,
        )
    });

    let summary = run(
        &ws,
        vec![
            FileOp::Rename {
                from: root.join("src/util.rs"),
                to: root.join("src/helpers.rs"),
                overwrite: false,
                ignore_if_exists: false,
            },
            edit_op(&main_rs, 0, 4, 8, "helpers"),
            edit_op(&other, 0, 3, 8, "renamed"),
        ],
        cx,
    )
    .expect("applied");
    assert!(summary.saved);
    assert_eq!(summary.files, 2);
    assert_eq!(
        fs::read_to_string(&main_rs).unwrap(),
        "mod helpers;\nfn main() {}\n",
        "the file opened for the edit is saved with the rename"
    );
    assert!(!editor(&ws, &main_rs, cx).read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(
        fs::read_to_string(&other).unwrap(),
        "// other\n",
        "a file with the user's unsaved changes is not saved"
    );
    assert!(other_editor.read_with(cx, |e, _| e.is_dirty()));
}

#[gpui::test]
fn deleting_a_file_with_unsaved_changes_is_refused(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let util = root.join("src/util.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&util, window, cx);
    });
    cx.run_until_parked();
    editor(&ws, &util, cx).update(cx, |e, cx| {
        e.apply_edits(
            vec![(
                arcode_buffer::Point::new(0, 0)..arcode_buffer::Point::new(0, 0),
                "x".into(),
            )],
            cx,
        )
    });
    let result = run(
        &ws,
        vec![
            edit_op(&main_rs, 0, 0, 9, ""),
            FileOp::Delete {
                path: util.clone(),
                recursive: false,
                ignore_if_not_exists: false,
            },
        ],
        cx,
    );
    assert!(result.unwrap_err().contains("unsaved changes"));
    assert!(util.exists(), "nothing was deleted");
    assert_eq!(
        fs::read_to_string(&main_rs).unwrap(),
        "mod util;\nfn main() {}\n"
    );
    let open_main = ws.read_with(cx, |ws, cx| {
        ws.editors(cx)
            .iter()
            .any(|e| e.read(cx).path() == Some(main_rs.as_path()))
    });
    assert!(!open_main, "the edit before the delete did not run");
}

#[gpui::test]
fn opening_files_for_an_edit_keeps_the_focus(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let util = root.join("src/util.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
        // Focus elsewhere (like the terminal).
        window.focus(&ws.focus_handle(cx));
    });
    cx.run_until_parked();
    // Two edits of one file and one of another: two files, each one step.
    let summary = run(
        &ws,
        vec![
            edit_op(&util, 0, 0, 0, "// a\n"),
            edit_op(&main_rs, 0, 0, 0, "// b\n"),
            edit_op(&util, 1, 7, 11, "tool"),
        ],
        cx,
    )
    .expect("applied");
    assert_eq!(summary.files, 2);
    assert_eq!(summary.opened, 1);
    assert_eq!(
        summary.message(Some("Rename")).as_deref(),
        Some("Rename: changed 2 files (1 opened unsaved)")
    );
    let focused = ws.update_in(cx, |ws, window, cx| ws.focus_handle(cx).is_focused(window));
    assert!(focused, "focus went back where it was");
    let util_editor = editor(&ws, &util, cx);
    assert_eq!(
        util_editor.read_with(cx, |e, _| e.buffer().to_string()),
        "// a\npub fn tool() {}\n"
    );
    util_editor.update(cx, |e, cx| e.undo(cx));
    assert_eq!(
        util_editor.read_with(cx, |e, _| e.buffer().to_string()),
        "pub fn util() {}\n"
    );
}

#[gpui::test]
fn a_rename_bound_to_fail_refuses_the_edits_before_it(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();
    // rust-analyzer sends the text edits first, then the file rename.
    let result = run(
        &ws,
        vec![
            edit_op(&main_rs, 0, 4, 8, "other"),
            FileOp::Rename {
                from: root.join("src/util.rs"),
                to: root.join("src/other.rs"),
                overwrite: false,
                ignore_if_exists: false,
            },
        ],
        cx,
    );
    assert!(result.unwrap_err().contains("already exists"));
    let main = editor(&ws, &main_rs, cx);
    assert_eq!(
        main.read_with(cx, |e, _| e.buffer().to_string()),
        "mod util;\nfn main() {}\n"
    );
    assert!(!main.read_with(cx, |e, _| e.is_dirty()));
}
