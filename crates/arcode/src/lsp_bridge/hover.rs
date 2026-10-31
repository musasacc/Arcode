//! The editor's [`HoverProvider`] backed by the document's language
//! servers: `textDocument/hover` for the popover and
//! `textDocument/definition` for the link drawn while Ctrl (Cmd) is held.

use arcode_buffer::Point;
use arcode_editor::{Editor, HoverInfo, HoverProvider};
use arcode_lsp::lsp_types::request::HoverRequest;
use arcode_lsp::lsp_types::{HoverParams, HoverProviderCapability, ServerCapabilities};
use gpui::{Context, Task, WeakEntity};

use super::LspBridge;
use super::convert::to_point;
use super::nav_convert::{goto_origin, hover_info};
use super::navigation::{GotoKind, text_position};

fn supports_hover(c: &ServerCapabilities) -> bool {
    matches!(
        c.hover_provider,
        Some(HoverProviderCapability::Simple(true)) | Some(HoverProviderCapability::Options(_))
    )
}

/// Hover and definition links from the bridge's language servers.
pub struct LspHoverProvider {
    bridge: WeakEntity<LspBridge>,
}

impl LspHoverProvider {
    pub fn new(bridge: WeakEntity<LspBridge>) -> Self {
        LspHoverProvider { bridge }
    }
}

impl HoverProvider for LspHoverProvider {
    fn hover(
        &self,
        editor: &Editor,
        at: Point,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<HoverInfo>>>> {
        let bridge = self.bridge.upgrade()?;
        let (manager, path) = bridge.read(cx).served(editor)?;
        let request =
            manager.request_with::<HoverRequest>(&path, supports_hover, |encoding| HoverParams {
                text_document_position_params: text_position(editor, &path, at, encoding),
                work_done_progress_params: Default::default(),
            });
        Some(cx.spawn(async move |this, cx| {
            let (server, hover) = request.await?;
            let Some(hover) = hover else {
                return Ok(None);
            };
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            this.update(cx, |editor, _| {
                hover_info(hover, |p| to_point(editor.buffer(), p, encoding))
            })
        }))
    }

    fn definition_link(
        &self,
        editor: &Editor,
        at: Point,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<std::ops::Range<Point>>>>> {
        let bridge = self.bridge.upgrade()?;
        let request = bridge.read(cx).goto_raw(editor, at, GotoKind::Definition)?;
        Some(cx.spawn(async move |this, cx| {
            let (response, encoding) = request.await?;
            let Some(origin) = goto_origin(&response) else {
                return Ok(None);
            };
            this.update(cx, |editor, _| match origin {
                Some(r) => Some(
                    to_point(editor.buffer(), r.start, encoding)
                        ..to_point(editor.buffer(), r.end, encoding),
                ),
                None => editor.word_at(at).map(|(range, _)| range),
            })
        }))
    }
}
