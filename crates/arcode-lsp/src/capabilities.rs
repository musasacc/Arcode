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
            "workspaceEdit": { "documentChanges": true },
            "symbol": { "symbolKind": { "valueSet": (1..=26).collect::<Vec<u32>>() } }
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
                    "commitCharactersSupport": true,
                    "documentationFormat": markup,
                    "deprecatedSupport": true,
                    "preselectSupport": true,
                    "insertReplaceSupport": true,
                    "labelDetailsSupport": true,
                    "resolveSupport": {
                        "properties": ["documentation", "detail", "additionalTextEdits"]
                    }
                },
                "completionItemKind": { "valueSet": (1..=25).collect::<Vec<u32>>() },
                "contextSupport": true
            },
            "definition": { "linkSupport": true },
            "typeDefinition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
            "documentHighlight": {},
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
                    "parameterInformation": { "labelOffsetSupport": true },
                    "activeParameterSupport": true
                },
                "contextSupport": true
            },
            "documentSymbol": {
                "hierarchicalDocumentSymbolSupport": true,
                "symbolKind": { "valueSet": (1..=26).collect::<Vec<u32>>() }
            }
        },
        "window": { "workDoneProgress": true },
        "general": { "positionEncodings": ["utf-8", "utf-16"] },
        // Client-side commands the editor runs when attached to a
        // completion item. rust-analyzer only attaches the ones listed here,
        // under its own name.
        "experimental": {
            "commands": {
                "commands": [
                    "editor.action.triggerParameterHints",
                    "rust-analyzer.triggerParameterHints"
                ]
            }
        }
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
        let commands = &caps.experimental.as_ref().unwrap()["commands"]["commands"];
        assert_eq!(
            commands,
            &json!([
                "editor.action.triggerParameterHints",
                "rust-analyzer.triggerParameterHints"
            ])
        );
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
        assert_eq!(item.insert_replace_support, Some(true));
        assert_eq!(item.commit_characters_support, Some(true));
        assert!(item.resolve_support.is_some());
        let completion = td.completion.as_ref().unwrap();
        assert_eq!(completion.context_support, Some(true));
        let kinds = completion.completion_item_kind.as_ref().unwrap();
        assert_eq!(kinds.value_set.as_ref().map(Vec::len), Some(25));
        let signature = td.signature_help.as_ref().unwrap();
        assert_eq!(signature.context_support, Some(true));
        let info = signature.signature_information.as_ref().unwrap();
        assert_eq!(info.active_parameter_support, Some(true));
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
        assert_eq!(
            td.type_definition.as_ref().unwrap().link_support,
            Some(true)
        );
        assert_eq!(td.implementation.as_ref().unwrap().link_support, Some(true));
        assert!(td.document_highlight.is_some());
        let symbol_kinds = td.document_symbol.as_ref().unwrap().symbol_kind.as_ref();
        assert_eq!(
            symbol_kinds.unwrap().value_set.as_ref().map(Vec::len),
            Some(26)
        );
        let ws = caps.workspace.as_ref().unwrap();
        assert_eq!(ws.configuration, Some(true));
        assert_eq!(ws.workspace_folders, Some(true));
        assert_eq!(ws.apply_edit, Some(true));
        assert!(ws.symbol.is_some());
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
