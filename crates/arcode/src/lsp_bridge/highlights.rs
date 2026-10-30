//! Occurrence highlights: `textDocument/documentHighlight` for the symbol
//! under the cursor, asked a moment after the cursor stops moving and
//! drawn as background highlights (set `"occurrences"`). Writes are a
//! little stronger than reads. Nothing is asked while text is selected,
//! with several cursors, off a word, or when `editor.occurrence_highlight`
//! is off.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use arcode_editor::{BackgroundHighlight, ColorKey, Editor};
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types::request::DocumentHighlightRequest;
use arcode_lsp::lsp_types::{DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams};
use arcode_ui_kit::settings;
use gpui::{Context, Entity, WeakEntity};

use super::LspBridge;
use super::convert::to_point;
use super::navigation::{one_of, text_position};

type HighlightAnswer = arcode_lsp::Result<(String, Option<Vec<DocumentHighlight>>)>;

/// Highlight set key.
pub const OCCURRENCES: &str = "occurrences";
/// Quiet time after a cursor move before asking.
const DELAY: Duration = Duration::from_millis(250);

impl LspBridge {
    /// Schedules (debounced) occurrence highlights for `editor`.
    pub(super) fn schedule_highlights(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let weak = editor.downgrade();
        let task = cx.spawn(async move |bridge, cx| {
            cx.background_executor().timer(DELAY).await;
            let Ok(Some(request)) = bridge.update(cx, |b, cx| b.highlight_request(&weak, cx))
            else {
                return;
            };
            let (version, encoding, answer) = request.await;
            weak.update(cx, |e, cx| {
                if e.buffer().version() != version {
                    return;
                }
                let highlights = answer
                    .into_iter()
                    .map(|h| {
                        let range = to_point(e.buffer(), h.range.start, encoding)
                            ..to_point(e.buffer(), h.range.end, encoding);
                        let strength = if h.kind == Some(DocumentHighlightKind::WRITE) {
                            0.6
                        } else {
                            0.35
                        };
                        BackgroundHighlight::text(
                            range,
                            ColorKey::new("selection").opacity(strength),
                        )
                    })
                    .collect();
                e.set_background_highlights(OCCURRENCES, highlights, cx);
            })
            .ok();
        });
        self.highlight_tasks.insert(editor.entity_id(), task);
    }

    /// The request for the editor's cursor, or `None` after clearing its
    /// highlights when there is nothing to ask.
    fn highlight_request(
        &mut self,
        weak: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> Option<impl Future<Output = (u64, PositionEncoding, Vec<DocumentHighlight>)> + use<>> {
        let editor = weak.upgrade()?;
        let served = {
            let e = editor.read(cx);
            let selections = e.selections();
            let on_word = selections.len() == 1
                && selections.primary().is_empty()
                && e.word_at(e.cursor_position()).is_some();
            let enabled = settings(cx).editor.occurrence_highlight;
            if on_word && enabled {
                self.served(e)
            } else {
                None
            }
        };
        let Some((manager, path)) = served else {
            editor.update(cx, |e, cx| e.clear_decorations(OCCURRENCES, cx));
            return None;
        };
        let e = editor.read(cx);
        let at = e.cursor_position();
        let version = e.buffer().version();
        let request: Pin<Box<dyn Future<Output = HighlightAnswer> + Send>> =
            Box::pin(manager.request_with::<DocumentHighlightRequest>(
                &path,
                |c| one_of(&c.document_highlight_provider),
                |encoding| DocumentHighlightParams {
                    text_document_position_params: text_position(e, &path, at, encoding),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                },
            ));
        Some(async move {
            match request.await {
                Ok((server, highlights)) => {
                    let encoding = manager
                        .position_encoding(&path, &server)
                        .unwrap_or_default();
                    (version, encoding, highlights.unwrap_or_default())
                }
                Err(_) => (version, Default::default(), Vec::new()),
            }
        })
    }
}
