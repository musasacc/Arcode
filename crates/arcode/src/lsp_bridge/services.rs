//! The editor's [`LanguageServices`] backed by the document's language
//! servers: `textDocument/inlayHint`, `codeAction` (+ `codeAction/resolve`
//! and `workspace/executeCommand`), `prepareRename` / `rename` and
//! `formatting` / `rangeFormatting`. Workspace edits go to the workspace
//! through [`LspBridge::apply_workspace_edit`]; the returned tasks finish
//! once they were applied.

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::Point;
use arcode_editor::{CodeAction, Editor, InlayHint, LanguageServices, RenameTarget, TextEdits};
use arcode_lsp::lsp_types::request::{
    CodeActionRequest, CodeActionResolveRequest, ExecuteCommand, Formatting, InlayHintRequest,
    PrepareRenameRequest, RangeFormatting, Rename,
};
use arcode_lsp::lsp_types::{
    CodeActionContext, CodeActionOrCommand, CodeActionParams, Command, DocumentFormattingParams,
    DocumentRangeFormattingParams, ExecuteCommandParams, FormattingOptions, InlayHintParams,
    Position, Range as LspRange, RenameParams, ServerCapabilities, TextDocumentIdentifier,
    WorkspaceEdit,
};
use arcode_lsp::{EditReply, LspManager, PositionEncoding, path_to_uri};
use arcode_ui_kit::settings;
use gpui::{AsyncApp, Context, Task, WeakEntity};

use super::LspBridge;
use super::convert::buffer_encoding;
use super::edits::{EditSource, edit_applied};
use super::navigation::{one_of, text_position};
use super::services_convert::{
    ActionData, code_action, context_diagnostics, inlay_hint, prepares_rename, rename_target,
    resolves_actions, supports_actions, supports_rename,
};
use super::workspace_edit::buffer_edits;

/// Inlay hints, code actions, rename and formatting from the bridge's
/// servers.
pub struct LspLanguageServices {
    bridge: WeakEntity<LspBridge>,
}

impl LspLanguageServices {
    pub fn new(bridge: WeakEntity<LspBridge>) -> Self {
        LspLanguageServices { bridge }
    }

    fn served(&self, editor: &Editor, cx: &gpui::App) -> Option<(Arc<LspManager>, PathBuf)> {
        self.bridge.upgrade()?.read(cx).served(editor)
    }

    /// `(start, end)` of `range` as an LSP range in `encoding`. A point
    /// past the last line (e.g. `Point::new(line_count, 0)` for "up to the
    /// end") means the end of the buffer, so the last line is covered.
    fn lsp_range(editor: &Editor, range: &Range<Point>, encoding: PositionEncoding) -> LspRange {
        let enc = buffer_encoding(encoding);
        let lines = editor.buffer().line_count();
        let clamp = |p: Point| {
            if p.line >= lines {
                let last = lines.saturating_sub(1);
                Point::new(last, editor.buffer().line_len(last))
            } else {
                p
            }
        };
        let (l0, c0) = editor.point_to_lsp(clamp(range.start), enc);
        let (l1, c1) = editor.point_to_lsp(clamp(range.end), enc);
        LspRange::new(Position::new(l0, c0), Position::new(l1, c1))
    }
}

/// Hands `edit` to the workspace and waits until it was applied.
async fn apply_edit(
    bridge: &WeakEntity<LspBridge>,
    edit: WorkspaceEdit,
    encoding: PositionEncoding,
    label: String,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    let (reply, outcome) = EditReply::channel();
    bridge.update(cx, |b, cx| {
        b.apply_workspace_edit(edit, encoding, Some(label), reply, EditSource::Editor, cx)
    })?;
    edit_applied(outcome).await
}

/// Runs `command` on `server` (`workspace/executeCommand`).
async fn execute(
    manager: &LspManager,
    path: &std::path::Path,
    server: &str,
    command: Command,
) -> anyhow::Result<()> {
    manager
        .request_to::<ExecuteCommand>(path, server, |_| ExecuteCommandParams {
            command: command.command,
            arguments: command.arguments.unwrap_or_default(),
            work_done_progress_params: Default::default(),
        })
        .await?;
    Ok(())
}

impl LanguageServices for LspLanguageServices {
    fn inlay_hints(
        &self,
        editor: &Editor,
        lines: Range<usize>,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<InlayHint>>>> {
        let (manager, path) = self.served(editor, cx)?;
        let end = Point::new(lines.end, 0);
        let range = Point::new(lines.start, 0)..end;
        let request = manager.request_with::<InlayHintRequest>(
            &path,
            |c| one_of(&c.inlay_hint_provider),
            |encoding| InlayHintParams {
                work_done_progress_params: Default::default(),
                text_document: TextDocumentIdentifier::new(path_to_uri(&path)),
                range: Self::lsp_range(editor, &range, encoding),
            },
        );
        let encoding_of = move |server: &str| manager.position_encoding(&path, server);
        Some(cx.spawn(async move |this, cx| {
            let (server, hints) = request.await?;
            let encoding = encoding_of(&server).unwrap_or_default();
            this.update(cx, |e, _| {
                hints
                    .unwrap_or_default()
                    .into_iter()
                    .map(|h| inlay_hint(e.buffer(), h, encoding))
                    .collect()
            })
        }))
    }

    fn code_actions(
        &self,
        editor: &Editor,
        range: Range<Point>,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<CodeAction>>>> {
        let bridge = self.bridge.upgrade()?;
        let (manager, path) = bridge.read(cx).served(editor)?;
        let store = bridge.read(cx).store();
        let mut requests = Vec::new();
        for server in manager.running_servers(&path) {
            let offers = manager
                .capabilities(&path, &server)
                .is_some_and(|c| supports_actions(&c));
            if !offers {
                continue;
            }
            let diagnostics: Vec<_> = store
                .for_path(&path)
                .into_iter()
                .filter(|(s, _)| *s == server)
                .map(|(_, d)| d)
                .collect();
            let request = manager.request_to::<CodeActionRequest>(&path, &server, |encoding| {
                let range = Self::lsp_range(editor, &range, encoding);
                CodeActionParams {
                    text_document: TextDocumentIdentifier::new(path_to_uri(&path)),
                    context: CodeActionContext {
                        diagnostics: context_diagnostics(diagnostics, &range),
                        only: None,
                        trigger_kind: None,
                    },
                    range,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }
            });
            requests.push((server, request));
        }
        if requests.is_empty() {
            return None;
        }
        Some(cx.spawn(async move |_, _| {
            let mut out = Vec::new();
            for (server, request) in requests {
                match request.await {
                    Ok(actions) => out.extend(
                        actions
                            .unwrap_or_default()
                            .into_iter()
                            .map(|a| code_action(&server, a)),
                    ),
                    Err(err) if err.is_cancellation() => {}
                    Err(err) => log::debug!("[{server}] codeAction failed: {err}"),
                }
            }
            Ok(out)
        }))
    }

    fn apply_code_action(
        &self,
        editor: &Editor,
        action: CodeAction,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        let data = action.data?.downcast::<ActionData>().ok()?;
        let (manager, path) = self.served(editor, cx)?;
        let bridge = self.bridge.clone();
        let title = action.title;
        let ActionData { server, raw } = (*data).clone();
        Some(cx.spawn(async move |_, cx| {
            let mut action = match raw {
                CodeActionOrCommand::Command(command) => {
                    return execute(&manager, &path, &server, command).await;
                }
                CodeActionOrCommand::CodeAction(action) => action,
            };
            let resolves = manager
                .capabilities(&path, &server)
                .is_some_and(|c| resolves_actions(&c));
            if action.edit.is_none() && resolves {
                action = manager
                    .request_to::<CodeActionResolveRequest>(&path, &server, |_| action)
                    .await?;
            }
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            if let Some(edit) = action.edit {
                apply_edit(&bridge, edit, encoding, title, cx).await?;
            }
            if let Some(command) = action.command {
                execute(&manager, &path, &server, command).await?;
            }
            Ok(())
        }))
    }

    fn prepare_rename(
        &self,
        editor: &Editor,
        at: Point,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<RenameTarget>>>> {
        let Some((manager, path)) = self.served(editor, cx) else {
            return Some(Task::ready(Err(anyhow::anyhow!(
                "no language server is running for this file"
            ))));
        };
        let offers = |f: fn(&ServerCapabilities) -> bool| {
            manager
                .running_servers(&path)
                .iter()
                .filter_map(|s| manager.capabilities(&path, s))
                .any(|c| f(&c))
        };
        if !offers(supports_rename) {
            return Some(Task::ready(Err(anyhow::anyhow!(
                "the language server cannot rename"
            ))));
        }
        if !offers(prepares_rename) {
            // Rename the word at the cursor.
            return None;
        }
        let request = manager.request_with::<PrepareRenameRequest>(&path, prepares_rename, |enc| {
            text_position(editor, &path, at, enc)
        });
        Some(cx.spawn(async move |this, cx| {
            let (server, response) = request.await?;
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            let Some(response) = response else {
                return Ok(None);
            };
            this.update(cx, |e, _| {
                rename_target(e.buffer(), response, encoding, || e.word_at(at))
            })
        }))
    }

    fn rename(
        &self,
        editor: &Editor,
        at: Point,
        new_name: String,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        let (manager, path) = self.served(editor, cx)?;
        let request = manager.request_with::<Rename>(&path, supports_rename, |enc| RenameParams {
            text_document_position: text_position(editor, &path, at, enc),
            new_name: new_name.clone(),
            work_done_progress_params: Default::default(),
        });
        let bridge = self.bridge.clone();
        Some(cx.spawn(async move |_, cx| {
            let (server, edit) = request.await?;
            let Some(edit) = edit else {
                anyhow::bail!("the language server found nothing to rename");
            };
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            apply_edit(&bridge, edit, encoding, format!("Rename to {new_name}"), cx).await
        }))
    }

    fn format(
        &self,
        editor: &Editor,
        range: Option<Range<Point>>,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<TextEdits>>> {
        let (manager, path) = self.served(editor, cx)?;
        let config = settings(cx);
        let language = editor.language().id.to_lowercase();
        let options = FormattingOptions {
            tab_size: config.effective_tab_size(&language) as u32,
            insert_spaces: config.effective_insert_spaces(&language),
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(true),
            trim_final_newlines: Some(true),
            ..Default::default()
        };
        let document = TextDocumentIdentifier::new(path_to_uri(&path));
        let supports_range = |c: &ServerCapabilities| one_of(&c.document_range_formatting_provider);
        let supports_document = |c: &ServerCapabilities| one_of(&c.document_formatting_provider);
        let request: std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>> = match range {
            Some(range) => {
                let offered = manager
                    .running_servers(&path)
                    .iter()
                    .filter_map(|s| manager.capabilities(&path, s))
                    .any(|c| supports_range(&c));
                if !offered {
                    return None;
                }
                Box::pin(
                    manager.request_with::<RangeFormatting>(&path, supports_range, |enc| {
                        DocumentRangeFormattingParams {
                            text_document: document.clone(),
                            range: Self::lsp_range(editor, &range, enc),
                            options: options.clone(),
                            work_done_progress_params: Default::default(),
                        }
                    }),
                )
            }
            None => {
                let offered = manager
                    .running_servers(&path)
                    .iter()
                    .filter_map(|s| manager.capabilities(&path, s))
                    .any(|c| supports_document(&c));
                if !offered {
                    return None;
                }
                Box::pin(
                    manager.request_with::<Formatting>(&path, supports_document, |_| {
                        DocumentFormattingParams {
                            text_document: document.clone(),
                            options: options.clone(),
                            work_done_progress_params: Default::default(),
                        }
                    }),
                )
            }
        };
        Some(cx.spawn(async move |this, cx| {
            let (server, edits) = request.await?;
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            let edits = edits.unwrap_or_default();
            this.update(cx, |e, _| buffer_edits(e.buffer(), &edits, encoding))?
                .map_err(anyhow::Error::msg)
        }))
    }
}

#[cfg(test)]
#[path = "review_services_tests.rs"]
mod review_tests;

/// The services object for an editor of the bridge.
pub(crate) fn services(bridge: WeakEntity<LspBridge>) -> Rc<dyn LanguageServices> {
    Rc::new(LspLanguageServices::new(bridge))
}
