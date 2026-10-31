//! [`Answer`]: when a single-answer request falls back to the next server.

use lsp_types::{
    DocumentHighlight, DocumentSymbolResponse, GotoDefinitionResponse, Hover, InlayHint, Location,
    PrepareRenameResponse, SignatureHelp, TextEdit, WorkspaceEdit,
};

/// A request result that can mean "nothing here", so a single-answer
/// request falls back to the next server; see
/// [`crate::LspManager::request_with`].
///
/// Implemented for the results of the requests the editor routes this way.
/// `null` is always "nothing". An empty list is "nothing" where servers use
/// it for "not found" (definition, references, symbols, inlay hints,
/// highlights, signatures) but an answer for formatting, where `[]` means
/// the document is already formatted.
pub trait Answer {
    /// True when the next server should be asked instead.
    fn is_null(&self) -> bool;
}

/// Whether a non-null result still means "nothing found".
trait Nothing {
    fn is_nothing(&self) -> bool {
        false
    }
}

impl<T: Nothing> Answer for Option<T> {
    fn is_null(&self) -> bool {
        self.as_ref().is_none_or(Nothing::is_nothing)
    }
}

impl Nothing for Hover {}
impl Nothing for PrepareRenameResponse {}
impl Nothing for WorkspaceEdit {}
impl Nothing for serde_json::Value {
    fn is_nothing(&self) -> bool {
        self.is_null()
    }
}

/// Formatting: `[]` is a real answer (nothing to change).
impl Nothing for Vec<TextEdit> {}

impl Nothing for Vec<Location> {
    fn is_nothing(&self) -> bool {
        self.is_empty()
    }
}

impl Nothing for Vec<InlayHint> {
    fn is_nothing(&self) -> bool {
        self.is_empty()
    }
}

impl Nothing for Vec<DocumentHighlight> {
    fn is_nothing(&self) -> bool {
        self.is_empty()
    }
}

impl Nothing for GotoDefinitionResponse {
    fn is_nothing(&self) -> bool {
        match self {
            GotoDefinitionResponse::Scalar(_) => false,
            GotoDefinitionResponse::Array(v) => v.is_empty(),
            GotoDefinitionResponse::Link(v) => v.is_empty(),
        }
    }
}

impl Nothing for DocumentSymbolResponse {
    fn is_nothing(&self) -> bool {
        match self {
            DocumentSymbolResponse::Flat(v) => v.is_empty(),
            DocumentSymbolResponse::Nested(v) => v.is_empty(),
        }
    }
}

impl Nothing for SignatureHelp {
    fn is_nothing(&self) -> bool {
        self.signatures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lists_fall_back_except_formatting() {
        assert!(None::<Hover>.is_null());
        assert!(Some(GotoDefinitionResponse::Array(Vec::new())).is_null());
        assert!(Some(GotoDefinitionResponse::Link(Vec::new())).is_null());
        assert!(Some(Vec::<Location>::new()).is_null());
        assert!(Some(DocumentSymbolResponse::Flat(Vec::new())).is_null());
        assert!(Some(Vec::<InlayHint>::new()).is_null());
        assert!(Some(Vec::<DocumentHighlight>::new()).is_null());
        assert!(Some(serde_json::Value::Null).is_null());
        // "Already formatted" stops the fallback.
        assert!(!Some(Vec::<TextEdit>::new()).is_null());
        assert!(!Some(WorkspaceEdit::default()).is_null());
        let help = SignatureHelp {
            signatures: Vec::new(),
            active_signature: None,
            active_parameter: None,
        };
        assert!(Some(help).is_null());
    }
}
