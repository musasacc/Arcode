//! Workspace edits applied through the bridge's events, as a server's
//! `workspace/applyEdit` would arrive (no real server runs in tests).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use arcode_lsp::lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
    Position, Range, RenameFile, ResourceOp, TextDocumentEdit, TextEdit, WorkspaceEdit,
};
use arcode_lsp::{EditOutcome, EditReply, LspEvent, PositionEncoding, path_to_uri};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

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
    fs::write(root.join("Cargo.toml"), "[package]\n").expect("write");
    fs::write(root.join("src/main.rs"), "fn main() { count(); }\n").expect("write");
    fs::write(root.join("src/lib.rs"), "pub fn count() {}\n").expect("write");
    (dir, root)
}

fn edit(line: u32, from: u32, to: u32, text: &str) -> TextEdit {
    TextEdit::new(
        Range::new(Position::new(line, from), Position::new(line, to)),
        text.into(),
    )
}

/// Sends `edit` as the server's `workspace/applyEdit` and returns what
/// the workspace answered.
fn apply(
    ws: &Entity<Workspace>,
    root: &Path,
    edit: WorkspaceEdit,
    cx: &mut VisualTestContext,
) -> EditOutcome {
    let (reply, mut rx) = EditReply::channel();
    let lsp = ws.read_with(cx, |ws, _| ws.lsp().clone());
    lsp.update(cx, |lsp, cx| {
        lsp.handle_event(
            LspEvent::ApplyEdit {
                server: "rust-analyzer".into(),
                root: root.to_path_buf(),
                encoding: PositionEncoding::Utf16,
                label: Some("Rename".into()),
                edit,
                reply,
            },
            cx,
        )
    });
    cx.run_until_parked();
    rx.try_recv().expect("answered")
}

fn text_of(ws: &Entity<Workspace>, path: &Path, cx: &mut VisualTestContext) -> Option<String> {
    ws.read_with(cx, |ws, cx| {
        ws.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(path))
            .map(|e| e.read(cx).buffer().to_string())
    })
}

fn active_path(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    ws.read_with(cx, |ws, cx| {
        ws.active_editor(cx)
            .and_then(|e| e.read(cx).path().map(Path::to_path_buf))
    })
}

#[gpui::test]
#[allow(clippy::mutable_key_type)] // `WorkspaceEdit::changes` is keyed by `Uri`.
fn edits_reach_open_and_unopened_files(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let lib_rs = root.join("src/lib.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();

    let mut changes = HashMap::new();
    changes.insert(path_to_uri(&main_rs), vec![edit(0, 12, 17, "total")]);
    changes.insert(path_to_uri(&lib_rs), vec![edit(0, 7, 12, "total")]);
    let outcome = apply(
        &ws,
        &root,
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        },
        cx,
    );
    assert!(outcome.applied, "{outcome:?}");
    assert_eq!(
        text_of(&ws, &main_rs, cx).as_deref(),
        Some("fn main() { total(); }\n")
    );
    assert_eq!(
        text_of(&ws, &lib_rs, cx).as_deref(),
        Some("pub fn total() {}\n"),
        "the unopened file was opened in a tab"
    );
    assert_eq!(active_path(&ws, cx), Some(main_rs.clone()), "focus stays");
    assert_eq!(
        fs::read_to_string(&lib_rs).unwrap(),
        "pub fn count() {}\n",
        "nothing is saved behind the user's back"
    );
    // One undo step per file.
    let lib = ws.read_with(cx, |ws, cx| {
        ws.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(lib_rs.as_path()))
            .unwrap()
    });
    assert!(lib.read_with(cx, |e, _| e.is_dirty()));
    lib.update(cx, |e, cx| e.undo(cx));
    assert_eq!(
        text_of(&ws, &lib_rs, cx).as_deref(),
        Some("pub fn count() {}\n")
    );
}

#[gpui::test]
fn stale_versions_refuse_the_whole_edit(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();
    let doc_edit = |version| {
        DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: path_to_uri(&main_rs),
                version: Some(version),
            },
            edits: vec![OneOf::Left(edit(0, 0, 2, "pub fn"))],
        })
    };
    let outcome = apply(
        &ws,
        &root,
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![doc_edit(999)])),
            ..Default::default()
        },
        cx,
    );
    assert!(!outcome.applied);
    assert!(outcome.failure_reason.unwrap().contains("changed"));
    assert_eq!(
        text_of(&ws, &main_rs, cx).as_deref(),
        Some("fn main() { count(); }\n")
    );
}

#[gpui::test]
fn renames_move_files_and_their_editors(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let lib_rs = root.join("src/lib.rs");
    let moved = root.join("src/counter.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&lib_rs, window, cx);
    });
    cx.run_until_parked();
    let outcome = apply(
        &ws,
        &root,
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
                    old_uri: path_to_uri(&lib_rs),
                    new_uri: path_to_uri(&moved),
                    options: None,
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: path_to_uri(&moved),
                        version: None,
                    },
                    edits: vec![OneOf::Left(edit(0, 0, 0, "// moved\n"))],
                }),
            ])),
            ..Default::default()
        },
        cx,
    );
    assert!(outcome.applied, "{outcome:?}");
    assert!(moved.exists() && !lib_rs.exists());
    assert_eq!(active_path(&ws, cx), Some(moved.clone()));
    assert_eq!(
        text_of(&ws, &moved, cx).as_deref(),
        Some("// moved\npub fn count() {}\n")
    );
}
