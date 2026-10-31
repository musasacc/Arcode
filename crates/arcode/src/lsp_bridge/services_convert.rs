//! Pure conversions for inlay hints, code actions, rename and formatting
//! between LSP types and the editor's.

use std::rc::Rc;

use arcode_buffer::{Buffer, Point};
use arcode_editor::{CodeAction, InlayHint, InlayHintKind, RenameTarget};
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::{
    self, CodeActionOrCommand, CodeActionProviderCapability, Diagnostic, InlayHintLabel, OneOf,
    Position, PrepareRenameResponse, Range, ServerCapabilities,
};

use super::convert::to_point;
use super::navigation::one_of;

/// Whether the server offers code actions.
pub fn supports_actions(c: &ServerCapabilities) -> bool {
    matches!(
        c.code_action_provider,
        Some(CodeActionProviderCapability::Simple(true))
            | Some(CodeActionProviderCapability::Options(_))
    )
}

/// Whether the server resolves code actions lazily.
pub fn resolves_actions(c: &ServerCapabilities) -> bool {
    matches!(
        &c.code_action_provider,
        Some(CodeActionProviderCapability::Options(o)) if o.resolve_provider == Some(true)
    )
}

/// Whether the server renames.
pub fn supports_rename(c: &ServerCapabilities) -> bool {
    one_of(&c.rename_provider)
}

/// Whether the server answers `prepareRename`.
pub fn prepares_rename(c: &ServerCapabilities) -> bool {
    matches!(&c.rename_provider, Some(OneOf::Right(o)) if o.prepare_provider == Some(true))
}

/// What the bridge keeps in [`CodeAction::data`] to apply an action.
#[derive(Debug, Clone)]
pub struct ActionData {
    /// The server that offered it.
    pub server: String,
    /// The action as the server sent it.
    pub raw: CodeActionOrCommand,
}

/// The editor's hint for an LSP inlay hint.
pub fn inlay_hint(
    buffer: &Buffer,
    hint: lsp_types::InlayHint,
    encoding: PositionEncoding,
) -> InlayHint {
    let label = match hint.label {
        InlayHintLabel::String(s) => s,
        InlayHintLabel::LabelParts(parts) => parts.into_iter().map(|p| p.value).collect(),
    };
    InlayHint {
        position: to_point(buffer, hint.position, encoding),
        label,
        kind: match hint.kind {
            Some(lsp_types::InlayHintKind::TYPE) => InlayHintKind::Type,
            Some(lsp_types::InlayHintKind::PARAMETER) => InlayHintKind::Parameter,
            _ => InlayHintKind::Other,
        },
        padding_left: hint.padding_left.unwrap_or(false),
        padding_right: hint.padding_right.unwrap_or(false),
    }
}

/// The editor's entry for a server's code action or command.
pub fn code_action(server: &str, raw: CodeActionOrCommand) -> CodeAction {
    let (title, kind, preferred, disabled) = match &raw {
        CodeActionOrCommand::Command(c) => (c.title.clone(), None, false, None),
        CodeActionOrCommand::CodeAction(a) => (
            a.title.clone(),
            a.kind.as_ref().map(|k| k.as_str().to_string()),
            a.is_preferred.unwrap_or(false),
            a.disabled.as_ref().map(|d| d.reason.clone()),
        ),
    };
    CodeAction {
        title,
        kind: kind.filter(|k| !k.is_empty()),
        preferred,
        disabled,
        data: Some(Rc::new(ActionData {
            server: server.to_string(),
            raw,
        })),
    }
}

/// Whether `a` and `b` overlap or touch (both in one encoding).
pub fn ranges_touch(a: &Range, b: &Range) -> bool {
    let key = |p: &Position| (p.line, p.character);
    key(&a.start) <= key(&b.end) && key(&b.start) <= key(&a.end)
}

/// The diagnostics of one server touching `range`, for a code action
/// request's context.
pub fn context_diagnostics<'a>(
    diagnostics: impl IntoIterator<Item = &'a Diagnostic>,
    range: &Range,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(|d| ranges_touch(&d.range, range))
        .cloned()
        .collect()
}

/// The rename target of a `prepareRename` answer. `word` is the word at
/// the position (used when the server only says "default behavior").
pub fn rename_target(
    buffer: &Buffer,
    response: PrepareRenameResponse,
    encoding: PositionEncoding,
    word: impl FnOnce() -> Option<(std::ops::Range<Point>, String)>,
) -> Option<RenameTarget> {
    let range = |r: Range| to_point(buffer, r.start, encoding)..to_point(buffer, r.end, encoding);
    let text = |r: &std::ops::Range<Point>| {
        let a = buffer.point_to_char(r.start);
        let b = buffer.point_to_char(r.end);
        buffer.slice_to_string(a..b)
    };
    match response {
        PrepareRenameResponse::Range(r) => {
            let range = range(r);
            let placeholder = text(&range);
            Some(RenameTarget { range, placeholder })
        }
        PrepareRenameResponse::RangeWithPlaceholder {
            range: r,
            placeholder,
        } => Some(RenameTarget {
            range: range(r),
            placeholder,
        }),
        PrepareRenameResponse::DefaultBehavior { .. } => {
            word().map(|(range, placeholder)| RenameTarget { range, placeholder })
        }
    }
}

#[cfg(test)]
#[path = "services_convert_tests.rs"]
mod tests;
