//! [`Features`]: which language features the client uses from one server.
//!
//! Several servers can serve one document (e.g. rust-analyzer plus a spell
//! checker). A feature turned off for a server is hidden from its
//! capabilities as soon as it is initialized, so every request routed by
//! capability skips it; turning off diagnostics drops what it publishes.

use lsp_types::ServerCapabilities;
use serde::{Deserialize, Serialize};

/// Per-server feature switches; everything is on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    /// Published (and pulled) diagnostics.
    pub diagnostics: bool,
    /// `textDocument/completion` (and resolve).
    pub completion: bool,
    /// `textDocument/hover`.
    pub hover: bool,
    /// `textDocument/signatureHelp`.
    pub signature_help: bool,
    /// Definition, declaration, type definition and implementation.
    pub definition: bool,
    /// `textDocument/references`.
    pub references: bool,
    /// Document and workspace symbols.
    pub symbols: bool,
    /// `textDocument/documentHighlight` (occurrences).
    pub highlights: bool,
    /// `prepareRename` / `rename`.
    pub rename: bool,
    /// Document, range and on-type formatting.
    pub formatting: bool,
    /// `textDocument/codeAction`.
    pub code_actions: bool,
    /// `textDocument/inlayHint`.
    pub inlay_hints: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features {
            diagnostics: true,
            completion: true,
            hover: true,
            signature_help: true,
            definition: true,
            references: true,
            symbols: true,
            highlights: true,
            rename: true,
            formatting: true,
            code_actions: true,
            inlay_hints: true,
        }
    }
}

impl Features {
    /// Removes the providers of every feature that is off.
    pub fn mask(&self, caps: &mut ServerCapabilities) {
        if !self.diagnostics {
            caps.diagnostic_provider = None;
        }
        if !self.completion {
            caps.completion_provider = None;
        }
        if !self.hover {
            caps.hover_provider = None;
        }
        if !self.signature_help {
            caps.signature_help_provider = None;
        }
        if !self.definition {
            caps.definition_provider = None;
            caps.declaration_provider = None;
            caps.type_definition_provider = None;
            caps.implementation_provider = None;
        }
        if !self.references {
            caps.references_provider = None;
        }
        if !self.symbols {
            caps.document_symbol_provider = None;
            caps.workspace_symbol_provider = None;
        }
        if !self.highlights {
            caps.document_highlight_provider = None;
        }
        if !self.rename {
            caps.rename_provider = None;
        }
        if !self.formatting {
            caps.document_formatting_provider = None;
            caps.document_range_formatting_provider = None;
            caps.document_on_type_formatting_provider = None;
        }
        if !self.code_actions {
            caps.code_action_provider = None;
        }
        if !self.inlay_hints {
            caps.inlay_hint_provider = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{CompletionOptions, HoverProviderCapability, OneOf};

    #[test]
    fn masking_hides_only_disabled_providers() {
        let mut caps = ServerCapabilities {
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            completion_provider: Some(CompletionOptions::default()),
            rename_provider: Some(OneOf::Left(true)),
            ..Default::default()
        };
        Features::default().mask(&mut caps);
        assert!(caps.hover_provider.is_some() && caps.completion_provider.is_some());
        let features = Features {
            hover: false,
            rename: false,
            ..Default::default()
        };
        features.mask(&mut caps);
        assert!(caps.hover_provider.is_none());
        assert!(caps.rename_provider.is_none());
        assert!(caps.completion_provider.is_some());
    }

    #[test]
    fn missing_keys_default_to_on() {
        let f: Features = serde_json::from_value(serde_json::json!({"hover": false})).unwrap();
        assert!(!f.hover);
        assert!(f.completion && f.diagnostics);
    }
}
