//! What Arcode tells servers it supports, and the `initialize` request.

use std::path::Path;

use lsp_types::{ClientCapabilities, ClientInfo, InitializeParams, WorkspaceFolder};
use serde_json::{Value, json};

use crate::uri::path_to_uri;

/// Client capabilities announced in `initialize`.
pub fn client_capabilities() -> ClientCapabilities {
    let markup = json!(["markdown", "plaintext"]);
    let caps = json!({
        "workspace": {
            "applyEdit": true,
            "configuration": true,
            "workspaceFolders": true,
            "workspaceEdit": { "documentChanges": true }
        },
        "textDocument": {
            "synchronization": { "didSave": true, "dynamicRegistration": false },
            "publishDiagnostics": {
                "relatedInformation": true,
                "versionSupport": true,
                "codeDescriptionSupport": true,
                "dataSupport": true
            },
            "hover": { "contentFormat": markup },
            "completion": {
                "completionItem": {
                    "snippetSupport": true,
                    "documentationFormat": markup,
                    "resolveSupport": {
                        "properties": ["documentation", "detail", "additionalTextEdits"]
                    }
                },
                "contextSupport": true
            },
            "definition": { "linkSupport": true },
            "references": {},
            "rename": { "prepareSupport": true },
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": [
                            "quickfix",
                            "refactor",
                            "refactor.extract",
                            "source.organizeImports"
                        ]
                    }
                },
                "resolveSupport": { "properties": ["edit"] }
            },
            "formatting": {},
            "inlayHint": {},
            "signatureHelp": {
                "signatureInformation": {
                    "documentationFormat": markup,
                    "parameterInformation": { "labelOffsetSupport": true }
                }
            },
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
        },
        "window": { "workDoneProgress": true },
        "general": { "positionEncodings": ["utf-8", "utf-16"] }
    });
    serde_json::from_value(caps).unwrap_or_default()
}

/// Builds the `initialize` parameters for a server rooted at `root`.
pub fn initialize_params(root: &Path, initialization_options: Option<Value>) -> InitializeParams {
    let root_uri = path_to_uri(root);
    let folder_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    #[allow(deprecated)]
    InitializeParams {
        process_id: Some(std::process::id()),
        root_uri: Some(root_uri.clone()),
        root_path: Some(root.to_string_lossy().into_owned()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri: root_uri,
            name: folder_name,
        }]),
        client_info: Some(ClientInfo {
            name: "Arcode".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        capabilities: client_capabilities(),
        initialization_options,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{CodeActionKind, MarkupKind, PositionEncodingKind};

    #[test]
    fn capabilities_parse_into_typed_struct() {
        let caps = client_capabilities();
        let td = caps.text_document.as_ref().unwrap();
        assert_eq!(td.synchronization.as_ref().unwrap().did_save, Some(true));
        let diag = td.publish_diagnostics.as_ref().unwrap();
        assert_eq!(diag.related_information, Some(true));
        assert_eq!(diag.version_support, Some(true));
        assert_eq!(diag.code_description_support, Some(true));
        let hover = td.hover.as_ref().unwrap();
        assert_eq!(
            hover.content_format.as_deref(),
            Some(&[MarkupKind::Markdown, MarkupKind::PlainText][..])
        );
        let item = td
            .completion
            .as_ref()
            .unwrap()
            .completion_item
            .as_ref()
            .unwrap();
        assert_eq!(item.snippet_support, Some(true));
        assert!(item.resolve_support.is_some());
        assert!(td.definition.is_some());
        assert!(td.references.is_some());
        assert_eq!(td.rename.as_ref().unwrap().prepare_support, Some(true));
        let kinds = &td
            .code_action
            .as_ref()
            .unwrap()
            .code_action_literal_support
            .as_ref()
            .unwrap()
            .code_action_kind
            .value_set;
        for k in [
            CodeActionKind::QUICKFIX,
            CodeActionKind::REFACTOR,
            CodeActionKind::REFACTOR_EXTRACT,
            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        ] {
            assert!(kinds.contains(&k.as_str().to_string()), "{k:?}");
        }
        assert!(td.formatting.is_some());
        assert!(td.inlay_hint.is_some());
        assert!(td.signature_help.is_some());
        assert!(td.document_symbol.is_some());
        let ws = caps.workspace.as_ref().unwrap();
        assert_eq!(ws.configuration, Some(true));
        assert_eq!(ws.workspace_folders, Some(true));
        assert_eq!(ws.apply_edit, Some(true));
        assert_eq!(caps.window.as_ref().unwrap().work_done_progress, Some(true));
        assert_eq!(
            caps.general.as_ref().unwrap().position_encodings,
            Some(vec![
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF16
            ])
        );
    }

    #[test]
    #[allow(deprecated)]
    fn initialize_params_carry_root_and_client_info() {
        let root = std::env::temp_dir().join("proj");
        let p = initialize_params(&root, Some(json!({"x": 1})));
        assert_eq!(p.process_id, Some(std::process::id()));
        assert_eq!(p.client_info.as_ref().unwrap().name, "Arcode");
        let folders = p.workspace_folders.as_ref().unwrap();
        assert_eq!(folders[0].name, "proj");
        assert_eq!(Some(&folders[0].uri), p.root_uri.as_ref());
        assert_eq!(p.initialization_options, Some(json!({"x": 1})));
    }
}
