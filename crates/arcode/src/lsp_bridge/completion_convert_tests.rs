use arcode_buffer::{Buffer, Point};
use arcode_editor::completion::CandidateKind;
use arcode_editor::{Documentation, ItemEdit};
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::{
    self, CompletionItemKind, CompletionList, CompletionResponse, CompletionTextEdit,
    InsertReplaceEdit, InsertTextFormat, MarkupContent, MarkupKind, ParameterInformation,
    ParameterLabel, Position, Range, SignatureInformation, TextEdit,
};

use super::*;

fn pos(line: u32, character: u32) -> Position {
    Position::new(line, character)
}

#[test]
fn items_convert_with_edits_snippets_and_docs() {
    let buffer = Buffer::new("use x;\nlet 😀 = v.pu\n");
    let lsp = lsp_types::CompletionItem {
        label: "push".into(),
        kind: Some(CompletionItemKind::METHOD),
        detail: Some("fn(&mut self, T)\nmore".into()),
        documentation: Some(lsp_types::Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "Appends **one**.".into(),
        })),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        text_edit: Some(CompletionTextEdit::InsertAndReplace(InsertReplaceEdit {
            new_text: "push(${1:v})".into(),
            // UTF-16: the emoji counts twice, so "pu" (char 10) is at 11.
            insert: Range::new(pos(1, 11), pos(1, 13)),
            replace: Range::new(pos(1, 11), pos(1, 13)),
        })),
        additional_text_edits: Some(vec![TextEdit::new(
            Range::new(pos(0, 0), pos(0, 0)),
            "use y;\n".into(),
        )]),
        commit_characters: Some(vec!["(".into()]),
        sort_text: Some("0001".into()),
        ..Default::default()
    };
    let item = completion_item(lsp, "ra", true, &buffer, PositionEncoding::Utf16);
    assert_eq!(item.candidate.kind, CandidateKind::Method);
    assert_eq!(item.candidate.detail.as_deref(), Some("fn(&mut self, T)"));
    assert_eq!(item.candidate.sort_text.as_deref(), Some("0001"));
    assert!(item.candidate.insert.is_snippet());
    assert_eq!(item.candidate.insert.text(), "push(${1:v})");
    assert_eq!(
        item.edit,
        Some(ItemEdit::new(Point::new(1, 10)..Point::new(1, 12)))
    );
    assert_eq!(
        item.additional_edits,
        vec![(Point::new(0, 0)..Point::new(0, 0), "use y;\n".to_string())]
    );
    assert_eq!(item.commit_characters, vec!["("]);
    assert_eq!(
        item.documentation,
        Some(Documentation::Markdown("Appends **one**.".into()))
    );
    assert!(item.needs_resolve);
    let data = item.provider_data.as_ref().unwrap();
    assert_eq!(data.downcast_ref::<LspItemData>().unwrap().server, "ra");
}

#[test]
fn plain_items_fall_back_to_insert_text_and_label() {
    let buffer = Buffer::new("");
    let lsp = lsp_types::CompletionItem {
        label: "len".into(),
        insert_text: Some("len()".into()),
        deprecated: Some(true),
        ..Default::default()
    };
    let item = completion_item(lsp, "s", false, &buffer, PositionEncoding::Utf8);
    assert_eq!(item.candidate.insert.text(), "len()");
    assert!(!item.candidate.insert.is_snippet());
    assert!(item.edit.is_none() && item.deprecated && !item.needs_resolve);
    assert_eq!(item.candidate.kind, CandidateKind::Text);
    let bare = completion_item(
        lsp_types::CompletionItem {
            label: "x".into(),
            ..Default::default()
        },
        "s",
        false,
        &buffer,
        PositionEncoding::Utf8,
    );
    assert_eq!(bare.candidate.insert.text(), "x");

    let (items, incomplete) = response_items(Some(CompletionResponse::List(CompletionList {
        is_incomplete: true,
        items: vec![Default::default()],
    })));
    assert_eq!((items.len(), incomplete), (1, true));
    assert_eq!(response_items(None).0.len(), 0);
}

#[test]
fn resolving_adds_docs_and_imports() {
    let buffer = Buffer::new("fn main() {}\n");
    let lsp = lsp_types::CompletionItem {
        label: "HashMap".into(),
        ..Default::default()
    };
    let item = completion_item(lsp.clone(), "ra", true, &buffer, PositionEncoding::Utf8);
    let resolved = lsp_types::CompletionItem {
        documentation: Some(lsp_types::Documentation::String("A map.".into())),
        detail: Some("struct HashMap".into()),
        additional_text_edits: Some(vec![TextEdit::new(
            Range::new(pos(0, 0), pos(0, 0)),
            "use std::collections::HashMap;\n".into(),
        )]),
        ..lsp
    };
    let merged = merge_resolved(item, resolved, &buffer, PositionEncoding::Utf8);
    assert_eq!(
        merged.documentation,
        Some(Documentation::PlainText("A map.".into()))
    );
    assert_eq!(merged.candidate.detail.as_deref(), Some("struct HashMap"));
    assert_eq!(merged.additional_edits.len(), 1);
    assert!(!merged.needs_resolve);
}

#[test]
fn signature_parameters_become_char_ranges() {
    let label = "fn größe(a: i32, b: &str)".to_string();
    let help = lsp_types::SignatureHelp {
        signatures: vec![SignatureInformation {
            label: label.clone(),
            documentation: None,
            parameters: Some(vec![
                ParameterInformation {
                    label: ParameterLabel::Simple("a: i32".into()),
                    documentation: None,
                },
                ParameterInformation {
                    // UTF-16 offsets.
                    label: ParameterLabel::LabelOffsets([17, 24]),
                    documentation: Some(lsp_types::Documentation::String("text".into())),
                },
            ]),
            active_parameter: None,
        }],
        active_signature: Some(5),
        active_parameter: Some(1),
    };
    let help = signature_help(help).unwrap();
    assert_eq!(help.active_signature, 0, "clamped");
    assert_eq!(help.active_parameter, Some(1));
    let sig = &help.signatures[0];
    let chars: Vec<char> = label.chars().collect();
    let text = |r: &std::ops::Range<usize>| chars[r.clone()].iter().collect::<String>();
    assert_eq!(text(&sig.parameters[0].label), "a: i32");
    assert_eq!(text(&sig.parameters[1].label), "b: &str");
    let empty = lsp_types::SignatureHelp {
        signatures: Vec::new(),
        active_signature: None,
        active_parameter: None,
    };
    assert!(signature_help(empty).is_none());
}

#[test]
fn signature_help_round_trips_for_active_signature_help() {
    let label = "fn größe(a: i32, b: &str)".to_string();
    let lsp = lsp_types::SignatureHelp {
        signatures: vec![SignatureInformation {
            label: label.clone(),
            documentation: Some(lsp_types::Documentation::String("doc".into())),
            parameters: Some(vec![ParameterInformation {
                label: ParameterLabel::LabelOffsets([17, 24]),
                documentation: None,
            }]),
            active_parameter: Some(0),
        }],
        active_signature: Some(0),
        active_parameter: Some(0),
    };
    let ours = signature_help(lsp.clone()).unwrap();
    let back = to_lsp_signature_help(&ours);
    assert_eq!(back, lsp);
}

#[test]
fn rust_analyzer_commands_map_to_editor_commands() {
    assert_eq!(
        editor_command("rust-analyzer.triggerParameterHints"),
        "editor.action.triggerParameterHints"
    );
    assert_eq!(
        editor_command("editor.action.triggerSuggest"),
        "editor.action.triggerSuggest"
    );
    assert_eq!(editor_command("x.run"), "x.run");
}
