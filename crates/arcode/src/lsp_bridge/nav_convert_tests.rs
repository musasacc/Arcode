//! Tests for [`super::nav_convert`].

use std::str::FromStr;

use arcode_buffer::Point;
use arcode_editor::Documentation;
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, GotoDefinitionResponse, Hover, HoverContents,
    LanguageString, Location, LocationLink, MarkedString, MarkupContent, MarkupKind, Position,
    Range, SymbolInformation, SymbolKind, Uri, WorkspaceSymbolResponse,
};

use super::nav_convert::*;
use crate::navigation::item::NavPos;

fn uri(path: &str) -> Uri {
    let base = if cfg!(windows) {
        "file:///C:"
    } else {
        "file://"
    };
    Uri::from_str(&format!("{base}{path}")).unwrap()
}

fn range(l0: u32, c0: u32, l1: u32, c1: u32) -> Range {
    Range::new(Position::new(l0, c0), Position::new(l1, c1))
}

fn location(path: &str, r: Range) -> Location {
    Location {
        uri: uri(path),
        range: r,
    }
}

#[test]
fn goto_answers_become_unique_targets() {
    let enc = PositionEncoding::Utf16;
    let a = location("/p/a.rs", range(3, 4, 3, 8));
    let targets = goto_targets(
        Some(GotoDefinitionResponse::Array(vec![a.clone(), a.clone()])),
        enc,
    );
    assert_eq!(targets.len(), 1);
    assert!(targets[0].path.ends_with("a.rs"));
    assert_eq!(targets[0].line(), 3);
    let link = LocationLink {
        origin_selection_range: Some(range(0, 1, 0, 5)),
        target_uri: uri("/p/b.rs"),
        target_range: range(10, 0, 20, 1),
        target_selection_range: range(10, 7, 10, 12),
    };
    let response = Some(GotoDefinitionResponse::Link(vec![link]));
    assert_eq!(goto_origin(&response), Some(Some(range(0, 1, 0, 5))));
    let targets = goto_targets(response, enc);
    assert_eq!(
        targets[0].pos,
        NavPos::Lsp {
            range: range(10, 7, 10, 12),
            encoding: enc
        }
    );
    assert_eq!(goto_origin(&None), None);
    assert_eq!(
        goto_origin(&Some(GotoDefinitionResponse::Array(vec![]))),
        None
    );
    assert_eq!(
        goto_origin(&Some(GotoDefinitionResponse::Scalar(a))),
        Some(None)
    );
}

#[test]
fn references_of_several_servers_are_merged() {
    let items = merged_reference_items(vec![
        (
            vec![
                location("/p/a.rs", range(2, 2, 2, 5)),
                location("/p/b.rs", range(1, 0, 1, 3)),
            ],
            PositionEncoding::Utf8,
        ),
        (
            vec![
                location("/p/a.rs", range(2, 2, 2, 5)),
                location("/p/a.rs", range(0, 1, 0, 2)),
            ],
            PositionEncoding::Utf16,
        ),
    ]);
    let lines: Vec<_> = items.iter().map(|i| i.target.line()).collect();
    assert_eq!(lines, [0, 2, 1], "a.rs:0, a.rs:2 (once), b.rs:1");
}

#[test]
fn references_are_sorted_by_file_and_position() {
    let items = reference_items(
        vec![
            location("/p/b.rs", range(1, 0, 1, 3)),
            location("/p/a.rs", range(9, 2, 9, 5)),
            location("/p/a.rs", range(2, 2, 2, 5)),
            location("/p/a.rs", range(2, 2, 2, 5)),
        ],
        PositionEncoding::Utf8,
    );
    let order: Vec<_> = items
        .iter()
        .map(|i| {
            (
                i.target
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                i.target.line(),
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![("a.rs".into(), 2), ("a.rs".into(), 9), ("b.rs".into(), 1)]
    );
}

#[test]
#[allow(deprecated)]
fn nested_document_symbols_flatten_with_depth() {
    let symbol = |name: &str, kind, r: Range, children| DocumentSymbol {
        name: name.into(),
        detail: Some(String::new()),
        kind,
        tags: None,
        deprecated: None,
        range: r,
        selection_range: r,
        children,
    };
    let response = DocumentSymbolResponse::Nested(vec![
        symbol("later", SymbolKind::FUNCTION, range(20, 0, 22, 1), None),
        symbol(
            "Point",
            SymbolKind::STRUCT,
            range(1, 0, 5, 1),
            Some(vec![symbol(
                "x",
                SymbolKind::FIELD,
                range(2, 4, 2, 5),
                None,
            )]),
        ),
    ]);
    let items = document_symbol_items(
        Some(response),
        std::path::Path::new("/p/a.rs"),
        PositionEncoding::Utf16,
    );
    let names: Vec<_> = items.iter().map(|i| (i.label.as_str(), i.depth)).collect();
    assert_eq!(names, vec![("Point", 0), ("x", 1), ("later", 0)]);
    assert_eq!(items[0].kind.unwrap().label, "struct");
    assert_eq!(items[0].detail, None, "empty details are dropped");
}

#[test]
#[allow(deprecated)]
fn workspace_symbols_show_container_and_file() {
    let info = SymbolInformation {
        name: "helper".into(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        location: location("/p/src/util.rs", range(4, 3, 4, 9)),
        container_name: Some("util".into()),
    };
    let items = workspace_symbol_items(
        Some(WorkspaceSymbolResponse::Flat(vec![info])),
        PositionEncoding::Utf16,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "helper");
    assert_eq!(items[0].detail.as_deref(), Some("util · util.rs"));
}

#[test]
fn hover_contents_become_documentation() {
    let hover = Hover {
        contents: HoverContents::Array(vec![
            MarkedString::LanguageString(LanguageString {
                language: "rust".into(),
                value: "fn helper()".into(),
            }),
            MarkedString::String("Helps.".into()),
            MarkedString::String("  ".into()),
        ]),
        range: Some(range(1, 4, 1, 10)),
    };
    let info = hover_info(hover, |p| Point::new(p.line as usize, p.character as usize)).unwrap();
    assert_eq!(
        info.contents,
        vec![
            Documentation::Markdown("```rust\nfn helper()\n```".into()),
            Documentation::Markdown("Helps.".into()),
        ]
    );
    assert_eq!(info.range, Some(Point::new(1, 4)..Point::new(1, 10)));
    let plain = Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::PlainText,
            value: "text".into(),
        }),
        range: None,
    };
    let (docs, r) = hover_docs(plain);
    assert_eq!(docs, vec![Documentation::PlainText("text".into())]);
    assert!(r.is_none());
    let empty = Hover {
        contents: HoverContents::Scalar(MarkedString::String(String::new())),
        range: None,
    };
    assert!(hover_info(empty, |_| Point::default()).is_none());
}
