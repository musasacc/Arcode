//! Pure conversions from LSP navigation answers (definitions, references,
//! symbols, hover) to the app's navigation and editor types.

use arcode_editor::{Documentation, HoverInfo};
use arcode_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, GotoDefinitionResponse, Hover, HoverContents, Location,
    LocationLink, MarkedString, MarkupKind, OneOf, SymbolInformation, SymbolKind,
    WorkspaceSymbolResponse,
};
use arcode_lsp::{PositionEncoding, uri_to_path};

use crate::navigation::item::{NavItem, NavPos, NavTarget, SymbolTag, lsp_symbol_tag};

/// The tag of a symbol; rust-analyzer reports `impl` blocks as objects.
fn symbol_tag(kind: SymbolKind, name: &str) -> Option<SymbolTag> {
    if kind == SymbolKind::OBJECT && name.starts_with("impl ") {
        return Some(SymbolTag {
            label: "impl",
            capture: "keyword",
        });
    }
    lsp_symbol_tag(kind)
}

/// A target for an LSP location (`None` for non-file URIs).
pub fn location_target(location: &Location, encoding: PositionEncoding) -> Option<NavTarget> {
    Some(NavTarget {
        path: uri_to_path(&location.uri)?,
        pos: NavPos::Lsp {
            range: location.range,
            encoding,
        },
    })
}

fn link_target(link: &LocationLink, encoding: PositionEncoding) -> Option<NavTarget> {
    Some(NavTarget {
        path: uri_to_path(&link.target_uri)?,
        pos: NavPos::Lsp {
            range: link.target_selection_range,
            encoding,
        },
    })
}

/// The targets of a definition, type definition or implementation answer,
/// without duplicates.
pub fn goto_targets(
    response: Option<GotoDefinitionResponse>,
    encoding: PositionEncoding,
) -> Vec<NavTarget> {
    let mut out: Vec<NavTarget> = match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(l)) => {
            location_target(&l, encoding).into_iter().collect()
        }
        Some(GotoDefinitionResponse::Array(ls)) => ls
            .iter()
            .filter_map(|l| location_target(l, encoding))
            .collect(),
        Some(GotoDefinitionResponse::Link(links)) => links
            .iter()
            .filter_map(|l| link_target(l, encoding))
            .collect(),
    };
    let mut seen = Vec::new();
    out.retain(|t| {
        let dup = seen.contains(t);
        if !dup {
            seen.push(t.clone());
        }
        !dup
    });
    out
}

/// The origin range a definition answer reports for the symbol under the
/// cursor (only `LocationLink`s carry one), if it has any target.
pub fn goto_origin(
    response: &Option<GotoDefinitionResponse>,
) -> Option<Option<arcode_lsp::lsp_types::Range>> {
    match response {
        None => None,
        Some(GotoDefinitionResponse::Scalar(_)) => Some(None),
        Some(GotoDefinitionResponse::Array(a)) if a.is_empty() => None,
        Some(GotoDefinitionResponse::Array(_)) => Some(None),
        Some(GotoDefinitionResponse::Link(links)) => {
            let first = links.first()?;
            Some(first.origin_selection_range)
        }
    }
}

/// Reference results as picker items, sorted by file then position. The
/// label is filled in by the preview loader.
pub fn reference_items(locations: Vec<Location>, encoding: PositionEncoding) -> Vec<NavItem> {
    let mut items: Vec<NavItem> = locations
        .iter()
        .filter_map(|l| location_target(l, encoding))
        .map(|t| NavItem::new(String::new(), t))
        .collect();
    items.sort_by(|a, b| {
        let key = |i: &NavItem| match &i.target.pos {
            NavPos::Lsp { range, .. } => (range.start.line, range.start.character),
            NavPos::Points(r) => (r.start.line as u32, r.start.column as u32),
        };
        (&a.target.path, key(a)).cmp(&(&b.target.path, key(b)))
    });
    items.dedup_by(|a, b| a.target == b.target);
    items
}

/// Document symbols, flattened in document order with nesting depths.
pub fn document_symbol_items(
    response: Option<DocumentSymbolResponse>,
    path: &std::path::Path,
    encoding: PositionEncoding,
) -> Vec<NavItem> {
    let mut out = Vec::new();
    match response {
        None => {}
        Some(DocumentSymbolResponse::Flat(symbols)) => {
            let mut symbols = symbols;
            symbols.sort_by_key(|s| {
                (
                    s.location.range.start.line,
                    s.location.range.start.character,
                )
            });
            out.extend(
                symbols
                    .iter()
                    .filter_map(|s| information_item(s, encoding, false)),
            );
        }
        Some(DocumentSymbolResponse::Nested(symbols)) => {
            flatten(&symbols, 0, path, encoding, &mut out);
        }
    }
    out
}

fn flatten(
    symbols: &[DocumentSymbol],
    depth: usize,
    path: &std::path::Path,
    encoding: PositionEncoding,
    out: &mut Vec<NavItem>,
) {
    let mut sorted: Vec<&DocumentSymbol> = symbols.iter().collect();
    sorted.sort_by_key(|s| (s.range.start.line, s.range.start.character));
    for symbol in sorted {
        out.push(NavItem {
            detail: symbol.detail.clone().filter(|d| !d.is_empty()),
            kind: symbol_tag(symbol.kind, &symbol.name),
            depth,
            ..NavItem::new(
                symbol.name.clone(),
                NavTarget {
                    path: path.to_path_buf(),
                    pos: NavPos::Lsp {
                        range: symbol.selection_range,
                        encoding,
                    },
                },
            )
        });
        if let Some(children) = &symbol.children {
            flatten(children, depth + 1, path, encoding, out);
        }
    }
}

fn information_item(
    symbol: &SymbolInformation,
    encoding: PositionEncoding,
    with_file: bool,
) -> Option<NavItem> {
    let target = location_target(&symbol.location, encoding)?;
    let file = with_file
        .then(|| {
            target
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .flatten();
    let detail = match (
        symbol.container_name.clone().filter(|c| !c.is_empty()),
        file,
    ) {
        (Some(c), Some(f)) => Some(format!("{c} · {f}")),
        (c, f) => c.or(f),
    };
    Some(NavItem {
        detail,
        kind: symbol_tag(symbol.kind, &symbol.name),
        ..NavItem::new(symbol.name.clone(), target)
    })
}

/// Workspace symbols (symbols without a range are left out).
pub fn workspace_symbol_items(
    response: Option<WorkspaceSymbolResponse>,
    encoding: PositionEncoding,
) -> Vec<NavItem> {
    match response {
        None => Vec::new(),
        Some(WorkspaceSymbolResponse::Flat(symbols)) => symbols
            .iter()
            .filter_map(|s| information_item(s, encoding, true))
            .collect(),
        Some(WorkspaceSymbolResponse::Nested(symbols)) => symbols
            .iter()
            .filter_map(|s| {
                let OneOf::Left(location) = &s.location else {
                    return None;
                };
                #[allow(deprecated)]
                let info = SymbolInformation {
                    name: s.name.clone(),
                    kind: s.kind,
                    tags: None,
                    deprecated: None,
                    location: location.clone(),
                    container_name: s.container_name.clone(),
                };
                information_item(&info, encoding, true)
            })
            .collect(),
    }
}

/// Hover contents as editor documentation; the range stays in server
/// coordinates (the caller converts it against the buffer).
pub fn hover_docs(hover: Hover) -> (Vec<Documentation>, Option<arcode_lsp::lsp_types::Range>) {
    let marked = |m: MarkedString| match m {
        MarkedString::String(s) => Documentation::Markdown(s),
        MarkedString::LanguageString(ls) => {
            Documentation::Markdown(format!("```{}\n{}\n```", ls.language, ls.value))
        }
    };
    let docs = match hover.contents {
        HoverContents::Scalar(m) => vec![marked(m)],
        HoverContents::Array(ms) => ms.into_iter().map(marked).collect(),
        HoverContents::Markup(m) => vec![match m.kind {
            MarkupKind::Markdown => Documentation::Markdown(m.value),
            MarkupKind::PlainText => Documentation::PlainText(m.value),
        }],
    };
    let docs = docs
        .into_iter()
        .filter(|d| !d.text().trim().is_empty())
        .collect();
    (docs, hover.range)
}

/// A [`HoverInfo`] whose range is converted with `to_point`.
pub fn hover_info(
    hover: Hover,
    to_point: impl Fn(arcode_lsp::lsp_types::Position) -> arcode_buffer::Point,
) -> Option<HoverInfo> {
    let (contents, range) = hover_docs(hover);
    if contents.is_empty() {
        return None;
    }
    Some(HoverInfo {
        contents,
        range: range.map(|r| to_point(r.start)..to_point(r.end)),
    })
}
