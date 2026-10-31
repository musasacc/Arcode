//! Pure conversions of LSP completion items and signature help into the
//! editor's types.

use std::ops::Range;
use std::rc::Rc;

use arcode_buffer::{Buffer, Point};
use arcode_editor::completion::{
    CandidateKind, CandidateSource, CompletionCandidate, InsertAction,
};
use arcode_editor::{
    CompletionItem, Documentation, ItemEdit, ParameterInfo, SignatureHelp, SignatureInfo,
};
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::{
    self, CompletionItemTag, CompletionResponse, CompletionTextEdit, InsertTextFormat, MarkupKind,
    ParameterLabel, TextEdit,
};

use super::convert::to_point;

/// What the editor keeps of an LSP item to resolve it later.
#[derive(Debug, Clone)]
pub struct LspItemData {
    /// The server that produced the item.
    pub server: String,
    /// The item as received (or as resolved).
    pub item: lsp_types::CompletionItem,
}

/// Items and incompleteness of one server's answer.
pub fn response_items(
    response: Option<CompletionResponse>,
) -> (Vec<lsp_types::CompletionItem>, bool) {
    match response {
        Some(CompletionResponse::Array(items)) => (items, false),
        Some(CompletionResponse::List(list)) => (list.items, list.is_incomplete),
        None => (Vec::new(), false),
    }
}

fn range(buffer: &Buffer, r: lsp_types::Range, encoding: PositionEncoding) -> Range<Point> {
    to_point(buffer, r.start, encoding)..to_point(buffer, r.end, encoding)
}

/// A server's command under the name the editor runs it by (servers name
/// their client-side commands differently).
pub fn editor_command(command: &str) -> String {
    match command {
        "rust-analyzer.triggerParameterHints" => "editor.action.triggerParameterHints",
        other => other,
    }
    .to_string()
}

/// LSP documentation as the editor's.
pub fn documentation(doc: lsp_types::Documentation) -> Option<Documentation> {
    let doc = match doc {
        lsp_types::Documentation::String(s) => Documentation::PlainText(s),
        lsp_types::Documentation::MarkupContent(m) if m.kind == MarkupKind::Markdown => {
            Documentation::Markdown(m.value)
        }
        lsp_types::Documentation::MarkupContent(m) => Documentation::PlainText(m.value),
    };
    (!doc.text().trim().is_empty()).then_some(doc)
}

fn text_edits(
    buffer: &Buffer,
    edits: Option<Vec<TextEdit>>,
    encoding: PositionEncoding,
) -> Vec<(Range<Point>, String)> {
    edits
        .unwrap_or_default()
        .into_iter()
        .map(|e| (range(buffer, e.range, encoding), e.new_text))
        .collect()
}

/// Converts one item from `server`. `resolvable` is whether the server
/// supports `completionItem/resolve`.
pub fn completion_item(
    lsp: lsp_types::CompletionItem,
    server: &str,
    resolvable: bool,
    buffer: &Buffer,
    encoding: PositionEncoding,
) -> CompletionItem {
    let kind = lsp
        .kind
        .and_then(|k| serde_json::to_value(k).ok())
        .and_then(|v| v.as_u64())
        .map_or(CandidateKind::Text, |k| CandidateKind::from_lsp(k as u32));
    let is_snippet = lsp.insert_text_format == Some(InsertTextFormat::SNIPPET);
    let (text, edit) = match lsp.text_edit.clone() {
        Some(CompletionTextEdit::Edit(e)) => (
            e.new_text,
            Some(ItemEdit::new(range(buffer, e.range, encoding))),
        ),
        Some(CompletionTextEdit::InsertAndReplace(e)) => (
            e.new_text,
            Some(ItemEdit {
                insert: range(buffer, e.insert, encoding),
                replace: range(buffer, e.replace, encoding),
            }),
        ),
        None => (
            lsp.insert_text.clone().unwrap_or_else(|| lsp.label.clone()),
            None,
        ),
    };
    let detail = lsp
        .label_details
        .as_ref()
        .and_then(|d| d.description.clone())
        .or_else(|| lsp.detail.clone())
        .filter(|d| !d.trim().is_empty());
    let mut candidate = CompletionCandidate::new(
        lsp.label.clone(),
        kind,
        CandidateSource::Lsp {
            server: server.to_string(),
        },
    );
    candidate.filter_text = lsp.filter_text.clone();
    candidate.sort_text = lsp.sort_text.clone();
    candidate.detail = detail.map(|d| d.lines().next().unwrap_or_default().to_string());
    candidate.preselect = lsp.preselect.unwrap_or(false);
    candidate.insert = if is_snippet {
        InsertAction::Snippet(text)
    } else {
        InsertAction::PlainText(text)
    };
    let deprecated = lsp.deprecated.unwrap_or(false)
        || lsp
            .tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED));
    let mut item = CompletionItem::new(candidate);
    item.edit = edit;
    item.additional_edits = text_edits(buffer, lsp.additional_text_edits.clone(), encoding);
    item.commit_characters = lsp.commit_characters.clone().unwrap_or_default();
    item.documentation = lsp.documentation.clone().and_then(documentation);
    item.deprecated = deprecated;
    item.command = lsp.command.as_ref().map(|c| editor_command(&c.command));
    item.needs_resolve = resolvable;
    item.provider_data = Some(Rc::new(LspItemData {
        server: server.to_string(),
        item: lsp,
    }));
    item
}

/// Merges a resolved LSP item into `item`: documentation, detail,
/// additional edits and the command.
pub fn merge_resolved(
    mut item: CompletionItem,
    resolved: lsp_types::CompletionItem,
    buffer: &Buffer,
    encoding: PositionEncoding,
) -> CompletionItem {
    if let Some(doc) = resolved.documentation.clone().and_then(documentation) {
        item.documentation = Some(doc);
    }
    if let Some(detail) = resolved.detail.clone().filter(|d| !d.trim().is_empty())
        && item.candidate.detail.is_none()
    {
        item.candidate.detail = Some(detail.lines().next().unwrap_or_default().to_string());
    }
    if resolved.additional_text_edits.is_some() {
        item.additional_edits =
            text_edits(buffer, resolved.additional_text_edits.clone(), encoding);
    }
    if let Some(command) = &resolved.command {
        item.command = Some(editor_command(&command.command));
    }
    item.needs_resolve = false;
    let server = item
        .provider_data
        .as_ref()
        .and_then(|d| d.downcast_ref::<LspItemData>())
        .map(|d| d.server.clone())
        .unwrap_or_default();
    item.provider_data = Some(Rc::new(LspItemData {
        server,
        item: resolved,
    }));
    item
}

/// Char range of a parameter label inside the signature label: a
/// substring, or UTF-16 offsets.
fn parameter_range(label: &str, param: &ParameterLabel) -> Range<usize> {
    match param {
        ParameterLabel::Simple(s) => match label.find(s.as_str()) {
            Some(byte) => {
                let start = label[..byte].chars().count();
                start..start + s.chars().count()
            }
            None => 0..0,
        },
        ParameterLabel::LabelOffsets([a, b]) => {
            let to_char = |units: u32| {
                let mut seen = 0u32;
                label
                    .chars()
                    .take_while(|c| {
                        seen += c.len_utf16() as u32;
                        seen <= units
                    })
                    .count()
            };
            to_char(*a)..to_char(*b)
        }
    }
}

/// LSP signature help as the editor's (`None` when it has no signature).
pub fn signature_help(help: lsp_types::SignatureHelp) -> Option<SignatureHelp> {
    if help.signatures.is_empty() {
        return None;
    }
    let signatures = help
        .signatures
        .into_iter()
        .map(|s| {
            let parameters = s
                .parameters
                .unwrap_or_default()
                .into_iter()
                .map(|p| ParameterInfo {
                    label: parameter_range(&s.label, &p.label),
                    documentation: p.documentation.and_then(documentation),
                })
                .collect();
            SignatureInfo {
                documentation: s.documentation.and_then(documentation),
                parameters,
                active_parameter: s.active_parameter.map(|a| a as usize),
                label: s.label,
            }
        })
        .collect::<Vec<_>>();
    let active_signature = help.active_signature.unwrap_or(0) as usize;
    Some(SignatureHelp {
        active_signature: active_signature.min(signatures.len() - 1),
        active_parameter: help.active_parameter.map(|a| a as usize),
        signatures,
    })
}

fn to_lsp_documentation(doc: &Documentation) -> lsp_types::Documentation {
    match doc {
        Documentation::PlainText(t) => lsp_types::Documentation::String(t.clone()),
        Documentation::Markdown(t) => {
            lsp_types::Documentation::MarkupContent(lsp_types::MarkupContent {
                kind: MarkupKind::Markdown,
                value: t.clone(),
            })
        }
    }
}

/// The editor's signature help back as LSP, for the `activeSignatureHelp`
/// of a retrigger. Parameter labels become UTF-16 offsets.
pub fn to_lsp_signature_help(help: &SignatureHelp) -> lsp_types::SignatureHelp {
    let signatures = help
        .signatures
        .iter()
        .map(|s| {
            let utf16 = |chars: usize| -> u32 {
                s.label
                    .chars()
                    .take(chars)
                    .map(|c| c.len_utf16() as u32)
                    .sum()
            };
            let parameters = s
                .parameters
                .iter()
                .map(|p| lsp_types::ParameterInformation {
                    label: ParameterLabel::LabelOffsets([utf16(p.label.start), utf16(p.label.end)]),
                    documentation: p.documentation.as_ref().map(to_lsp_documentation),
                })
                .collect();
            lsp_types::SignatureInformation {
                label: s.label.clone(),
                documentation: s.documentation.as_ref().map(to_lsp_documentation),
                parameters: Some(parameters),
                active_parameter: s.active_parameter.map(|a| a as u32),
            }
        })
        .collect();
    lsp_types::SignatureHelp {
        signatures,
        active_signature: Some(help.active_signature as u32),
        active_parameter: help.active_parameter.map(|a| a as u32),
    }
}

#[cfg(test)]
#[path = "completion_convert_tests.rs"]
mod tests;
