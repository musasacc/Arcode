//! Tests for [`super::services_convert`] and [`super::super::workspace_edit`].

use std::collections::HashMap;
use std::str::FromStr;

use arcode_buffer::{Buffer, Point};
use arcode_editor::InlayHintKind;
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::{
    self, AnnotatedTextEdit, CodeActionDisabled, CodeActionKind, CodeActionOrCommand, Command,
    CreateFile, CreateFileOptions, DeleteFile, DocumentChangeOperation, DocumentChanges,
    InlayHintLabel, InlayHintLabelPart, OneOf, OptionalVersionedTextDocumentIdentifier, Position,
    PrepareRenameResponse, Range, RenameFile, ResourceOp, TextDocumentEdit, TextEdit, Uri,
    WorkspaceEdit,
};

use super::*;
use crate::lsp_bridge::workspace_edit::{FileOp, buffer_edits, files_touched, plan};

fn uri(path: &str) -> Uri {
    let base = if cfg!(windows) {
        "file:///C:"
    } else {
        "file://"
    };
    Uri::from_str(&format!("{base}{path}")).unwrap()
}

fn path(p: &str) -> std::path::PathBuf {
    arcode_lsp::uri_to_path(&uri(p)).unwrap()
}

fn range(l0: u32, c0: u32, l1: u32, c1: u32) -> Range {
    Range::new(Position::new(l0, c0), Position::new(l1, c1))
}

#[test]
fn inlay_hints_join_label_parts_and_map_kinds() {
    let buffer = Buffer::new("let ü = f(1);\n");
    let hint = lsp_types::InlayHint {
        position: Position::new(0, 5),
        label: InlayHintLabel::LabelParts(vec![
            InlayHintLabelPart {
                value: ": Vec<".into(),
                ..Default::default()
            },
            InlayHintLabelPart {
                value: "i32>".into(),
                ..Default::default()
            },
        ]),
        kind: Some(lsp_types::InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: Some(true),
        data: None,
    };
    let h = inlay_hint(&buffer, hint, PositionEncoding::Utf8);
    // Byte 5 is after the two-byte `ü`: char column 5 - 1.
    assert_eq!(h.position, Point::new(0, 4));
    assert_eq!(h.label, ": Vec<i32>");
    assert_eq!(h.kind, InlayHintKind::Type);
    assert!(!h.padding_left && h.padding_right);
}

#[test]
fn code_actions_keep_their_server_and_kind() {
    let action = lsp_types::CodeAction {
        title: "Extract into function".into(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        is_preferred: Some(true),
        disabled: Some(CodeActionDisabled {
            reason: "select an expression".into(),
        }),
        ..Default::default()
    };
    let a = code_action("ra", CodeActionOrCommand::CodeAction(action));
    assert_eq!(a.title, "Extract into function");
    assert_eq!(a.kind.as_deref(), Some("refactor.extract"));
    assert!(a.preferred);
    assert_eq!(a.disabled.as_deref(), Some("select an expression"));
    let data = a.data.unwrap().downcast::<ActionData>().ok().unwrap();
    assert_eq!(data.server, "ra");

    let command = Command::new("Run".into(), "run.it".into(), None);
    let c = code_action("ra", CodeActionOrCommand::Command(command));
    assert_eq!(c.kind, None);
    assert!(!c.preferred && c.disabled.is_none());
}

#[test]
fn context_diagnostics_touch_the_range() {
    let diag = |r: Range| lsp_types::Diagnostic {
        range: r,
        message: "m".into(),
        ..Default::default()
    };
    let all = [
        diag(range(1, 0, 1, 4)),
        diag(range(3, 2, 3, 6)),
        diag(range(0, 0, 0, 1)),
    ];
    let picked = context_diagnostics(&all, &range(1, 4, 1, 4));
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].range, range(1, 0, 1, 4));
    assert_eq!(context_diagnostics(&all, &range(0, 0, 9, 0)).len(), 3);
}

#[test]
fn prepare_rename_answers_become_targets() {
    let buffer = Buffer::new("fn count() {}\n");
    let word = || Some((Point::new(0, 3)..Point::new(0, 8), "count".to_string()));
    let utf16 = PositionEncoding::Utf16;
    let t = rename_target(&buffer, PrepareRenameResponse::Range(range(0, 3, 0, 8)), utf16, word)
        .unwrap();
    assert_eq!((t.range.start, t.placeholder.as_str()), (Point::new(0, 3), "count"));
    let t = rename_target(
        &buffer,
        PrepareRenameResponse::RangeWithPlaceholder {
            range: range(0, 3, 0, 8),
            placeholder: "Count".into(),
        },
        utf16,
        word,
    )
    .unwrap();
    assert_eq!(t.placeholder, "Count");
    let t = rename_target(
        &buffer,
        PrepareRenameResponse::DefaultBehavior {
            default_behavior: true,
        },
        utf16,
        word,
    )
    .unwrap();
    assert_eq!(t.range.end, Point::new(0, 8));
}

#[test]
fn plain_changes_are_ordered_by_path() {
    let mut changes = HashMap::new();
    changes.insert(uri("/p/b.rs"), vec![TextEdit::new(range(0, 0, 0, 1), "x".into())]);
    changes.insert(uri("/p/a.rs"), vec![TextEdit::new(range(1, 0, 1, 0), "y".into())]);
    let ops = plan(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].edited_path(), Some(&path("/p/a.rs")));
    assert_eq!(files_touched(&ops), 2);
}

#[test]
fn document_changes_keep_order_and_resource_operations() {
    let doc_edit = TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: uri("/p/new.rs"),
            version: Some(4),
        },
        edits: vec![
            OneOf::Left(TextEdit::new(range(0, 0, 0, 0), "a".into())),
            OneOf::Right(AnnotatedTextEdit {
                text_edit: TextEdit::new(range(1, 0, 1, 0), "b".into()),
                annotation_id: "x".into(),
            }),
        ],
    };
    let ops = plan(WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: uri("/p/new.rs"),
                options: Some(CreateFileOptions {
                    overwrite: None,
                    ignore_if_exists: Some(true),
                }),
                annotation_id: None,
            })),
            DocumentChangeOperation::Edit(doc_edit),
            DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
                old_uri: uri("/p/old.rs"),
                new_uri: uri("/p/moved.rs"),
                options: None,
                annotation_id: None,
            })),
            DocumentChangeOperation::Op(ResourceOp::Delete(DeleteFile {
                uri: uri("/p/gone"),
                options: None,
            })),
        ])),
        // Ignored when documentChanges is present.
        changes: Some(HashMap::from([(uri("/p/z.rs"), vec![])])),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(ops.len(), 4);
    assert!(matches!(&ops[0], FileOp::Create { ignore_if_exists: true, overwrite: false, .. }));
    match &ops[1] {
        FileOp::Edit { version, edits, .. } => {
            assert_eq!(*version, Some(4));
            assert_eq!(edits[1].new_text, "b");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&ops[2], FileOp::Rename { to, .. } if *to == path("/p/moved.rs")));
    assert!(matches!(&ops[3], FileOp::Delete { recursive: false, .. }));
}

#[test]
fn text_edits_become_sorted_points_and_reject_overlaps() {
    let buffer = Buffer::new("héllo\nworld\n");
    let edits = vec![
        TextEdit::new(range(1, 0, 1, 5), "earth".into()),
        TextEdit::new(range(0, 3, 0, 3), "!".into()),
    ];
    let out = buffer_edits(&buffer, &edits, PositionEncoding::Utf8).unwrap();
    // UTF-8 byte 3 is after "hé".
    assert_eq!(out[0].0, Point::new(0, 2)..Point::new(0, 2));
    assert_eq!(out[1].1, "earth");
    let overlapping = vec![
        TextEdit::new(range(0, 0, 0, 4), "a".into()),
        TextEdit::new(range(0, 2, 0, 5), "b".into()),
    ];
    assert!(buffer_edits(&buffer, &overlapping, PositionEncoding::Utf16).is_err());
}
