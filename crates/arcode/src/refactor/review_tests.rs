//! Review tests for applying workspace edits. Each reproduces a bug found
//! in review and fails until it is fixed.

use std::fs;
use std::path::{Path, PathBuf};

use arcode_lsp::lsp_types::{
    CreateFile, CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextEdit, WorkspaceEdit,
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
    (dir, root)
}

fn edit(line: u32, from: u32, to: u32, text: &str) -> TextEdit {
    TextEdit::new(
        Range::new(Position::new(line, from), Position::new(line, to)),
        text.into(),
    )
}

fn doc_edit(path: &Path, edits: Vec<TextEdit>) -> DocumentChangeOperation {
    DocumentChangeOperation::Edit(TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: path_to_uri(path),
            version: None,
        },
        edits: edits.into_iter().map(OneOf::Left).collect(),
    })
}

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
                label: Some("Fix".into()),
                edit,
                reply,
            },
            cx,
        )
    });
    cx.run_until_parked();
    rx.try_recv().expect("answered")
}

fn editor_text(ws: &Entity<Workspace>, path: &Path, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(path))
            .map(|e| e.read(cx).buffer().to_string())
            .expect("open editor")
    })
}

/// `documentChanges` may hold several `TextDocumentEdit`s for one file
/// (the spec allows it; e.g. an action that edits imports and a body
/// separately). Each one becomes its own `apply_edits` call, so the file
/// gets one undo step per entry instead of one per file, and the summary
/// counts the file twice.
#[gpui::test]
fn several_document_edits_of_one_file_are_one_undo_step(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();

    let outcome = apply(
        &ws,
        &root,
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                doc_edit(&main_rs, vec![edit(0, 0, 0, "use x;\n")]),
                doc_edit(&main_rs, vec![edit(1, 12, 17, "total")]),
            ])),
            ..Default::default()
        },
        cx,
    );
    assert!(outcome.applied, "{outcome:?}");
    assert_eq!(
        editor_text(&ws, &main_rs, cx),
        "use x;\nfn main() { total(); }\n"
    );
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("active"));
    editor.update(cx, |e, cx| e.undo(cx));
    assert_eq!(
        editor_text(&ws, &main_rs, cx),
        "fn main() { count(); }\n",
        "one undo must revert the whole edit of this file"
    );
}

/// A `CreateFile` with `overwrite` on a file that is open empties the file
/// on disk only; the open editor keeps the old text, so the following text
/// edit (computed against the now empty file) is applied on top of the old
/// content.
#[gpui::test]
fn overwrite_create_of_an_open_file_then_edit(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let main_rs = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();

    let outcome = apply(
        &ws,
        &root,
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: path_to_uri(&main_rs),
                    options: Some(CreateFileOptions {
                        overwrite: Some(true),
                        ignore_if_exists: None,
                    }),
                    annotation_id: None,
                })),
                doc_edit(&main_rs, vec![edit(0, 0, 0, "fn main() {}\n")]),
            ])),
            ..Default::default()
        },
        cx,
    );
    assert!(outcome.applied, "{outcome:?}");
    assert_eq!(
        editor_text(&ws, &main_rs, cx),
        "fn main() {}\n",
        "the file was recreated empty before the edit"
    );
}
