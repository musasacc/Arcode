//! The editor's [`CompletionProvider`] backed by the language servers of
//! the document: `textDocument/completion` (fanned out to every server
//! that offers it), `completionItem/resolve` (to the server that produced
//! the item) and `textDocument/signatureHelp`.
//!
//! Requests are sent from a task, so the `didChange` for the keystroke
//! that caused them has already been queued; answers are converted against
//! the buffer when they arrive.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcode_editor::{
    CompletionItem, CompletionList, CompletionProvider, CompletionTrigger, Editor, SignatureHelp,
    SignatureRequest, SignatureTriggers,
};
use arcode_lsp::lsp_types::request::{Completion, ResolveCompletionItem, SignatureHelpRequest};
use arcode_lsp::lsp_types::{
    CompletionContext, CompletionParams, CompletionTriggerKind, ServerCapabilities,
    SignatureHelpContext, SignatureHelpParams, SignatureHelpTriggerKind, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use arcode_lsp::{LspManager, PositionEncoding, path_to_uri};
use gpui::{App, AppContext as _, Context, Task, WeakEntity};

use super::LspBridge;
use super::completion_convert::{
    LspItemData, completion_item, merge_resolved, response_items, signature_help,
};
use super::convert::buffer_encoding;

/// Completion and signature help from the bridge's language servers.
pub struct LspCompletionProvider {
    bridge: WeakEntity<LspBridge>,
}

impl LspCompletionProvider {
    pub fn new(bridge: WeakEntity<LspBridge>) -> Self {
        LspCompletionProvider { bridge }
    }

    /// The manager and the document path, when a server runs for it.
    fn target(&self, editor: &Editor, cx: &App) -> Option<(Arc<LspManager>, PathBuf)> {
        let bridge = self.bridge.upgrade()?;
        let manager = bridge.read(cx).manager.clone()?;
        let path = editor.path()?.to_path_buf();
        (!manager.running_servers(&path).is_empty()).then_some((manager, path))
    }

    /// Capabilities of every running server for `path`.
    fn capabilities(manager: &LspManager, path: &Path) -> Vec<ServerCapabilities> {
        manager
            .running_servers(path)
            .iter()
            .filter_map(|s| manager.capabilities(path, s))
            .collect()
    }
}

/// The text document position of the primary cursor in `encoding`.
fn position(
    editor: &Editor,
    path: &Path,
    encoding: PositionEncoding,
) -> TextDocumentPositionParams {
    let head = editor.selections().primary().head;
    let (line, character) = editor
        .buffer()
        .to_lsp_position(head, buffer_encoding(encoding));
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: path_to_uri(path),
        },
        position: arcode_lsp::lsp_types::Position::new(line, character),
    }
}

impl CompletionProvider for LspCompletionProvider {
    fn trigger_characters(&self, editor: &Editor, cx: &App) -> Vec<String> {
        let Some((manager, path)) = self.target(editor, cx) else {
            return Vec::new();
        };
        Self::capabilities(&manager, &path)
            .into_iter()
            .filter_map(|c| c.completion_provider?.trigger_characters)
            .flatten()
            .collect()
    }

    fn signature_triggers(&self, editor: &Editor, cx: &App) -> SignatureTriggers {
        let Some((manager, path)) = self.target(editor, cx) else {
            return SignatureTriggers::default();
        };
        let mut out = SignatureTriggers {
            trigger: Vec::new(),
            retrigger: Vec::new(),
        };
        for caps in Self::capabilities(&manager, &path) {
            if let Some(sig) = caps.signature_help_provider {
                out.trigger
                    .extend(sig.trigger_characters.unwrap_or_default());
                out.retrigger
                    .extend(sig.retrigger_characters.unwrap_or_default());
            }
        }
        if out.trigger.is_empty() {
            return SignatureTriggers::default();
        }
        out
    }

    fn completions(
        &self,
        editor: &Editor,
        trigger: &CompletionTrigger,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionList>>> {
        let (manager, path) = self.target(editor, cx)?;
        let context = match trigger {
            CompletionTrigger::Invoked => CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            },
            CompletionTrigger::Character(c) => CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(c.clone()),
            },
            CompletionTrigger::Incomplete => CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
                trigger_character: None,
            },
        };
        let request = manager.request_all_with::<Completion>(
            &path,
            |c| c.completion_provider.is_some(),
            |encoding| CompletionParams {
                text_document_position: position(editor, &path, encoding),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: Some(context.clone()),
            },
        );
        Some(cx.spawn(async move |this, cx| {
            let answers = request.await;
            this.update(cx, |editor, _| {
                let mut list = CompletionList::default();
                for (server, response) in answers {
                    let encoding = manager
                        .position_encoding(&path, &server)
                        .unwrap_or_default();
                    let resolvable = manager
                        .capabilities(&path, &server)
                        .and_then(|c| c.completion_provider?.resolve_provider)
                        .unwrap_or(false);
                    let (items, incomplete) = response_items(response);
                    list.is_incomplete |= incomplete;
                    list.items.extend(items.into_iter().map(|item| {
                        completion_item(item, &server, resolvable, editor.buffer(), encoding)
                    }));
                }
                list
            })
        }))
    }

    fn resolve(
        &self,
        editor: &Editor,
        item: &CompletionItem,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionItem>>> {
        let (manager, path) = self.target(editor, cx)?;
        let data = item
            .provider_data
            .as_ref()?
            .downcast_ref::<LspItemData>()?
            .clone();
        let request =
            manager.request_to::<ResolveCompletionItem>(&path, &data.server, |_| data.item.clone());
        let item = item.clone();
        Some(cx.spawn(async move |this, cx| {
            let resolved = request.await?;
            this.update(cx, |editor, _| {
                let encoding = manager
                    .position_encoding(&path, &data.server)
                    .unwrap_or_default();
                merge_resolved(item, resolved, editor.buffer(), encoding)
            })
        }))
    }

    fn signature_help(
        &self,
        editor: &Editor,
        request: &SignatureRequest,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<SignatureHelp>>>> {
        let (manager, path) = self.target(editor, cx)?;
        let trigger_kind = if request.trigger_character.is_some() {
            SignatureHelpTriggerKind::TRIGGER_CHARACTER
        } else if request.is_retrigger {
            SignatureHelpTriggerKind::CONTENT_CHANGE
        } else {
            SignatureHelpTriggerKind::INVOKED
        };
        let context = SignatureHelpContext {
            trigger_kind,
            trigger_character: request.trigger_character.clone(),
            is_retrigger: request.is_retrigger,
            active_signature_help: None,
        };
        let request = manager.request_with::<SignatureHelpRequest>(
            &path,
            |c| c.signature_help_provider.is_some(),
            |encoding| SignatureHelpParams {
                context: Some(context),
                text_document_position_params: position(editor, &path, encoding),
                work_done_progress_params: Default::default(),
            },
        );
        Some(cx.background_spawn(async move {
            let (_, help) = request.await?;
            Ok(help.and_then(signature_help))
        }))
    }
}
