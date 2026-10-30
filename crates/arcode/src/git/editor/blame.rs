//! Inline blame for the cursor line and the blame column.

use std::sync::Arc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{ColorKey, Editor, VirtualText};
use arcode_git::BlameLine;
use arcode_ui_kit::settings;
use gpui::{AppContext, AsyncApp, Context, Entity, EntityId, WeakEntity};

use super::{BLAME_COLUMN_SET, BLAME_SET, GitEditors};
use crate::git::blame_text::{column_text, inline_text, now};

/// Quiet time after an edit before the blame column is recomputed.
const COLUMN_DEBOUNCE: Duration = Duration::from_millis(600);
/// Files with more lines than this get no blame column.
const MAX_COLUMN_LINES: usize = 50_000;

impl GitEditors {
    /// Shows blame for the cursor line of `id` after the configured delay.
    pub(crate) fn schedule_inline_blame(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let blame_settings = settings(cx).git.clone();
        let column = self.blame_column;
        let Some(state) = self.editors.get_mut(&id) else {
            return;
        };
        let Some(editor) = state.editor.upgrade() else {
            return;
        };
        if !blame_settings.inline_blame || column || state.repo.is_none() {
            state.blame_task = None;
            return;
        }
        let line = editor.read(cx).cursor_position().line;
        if state.blame_shown == Some(line) {
            return;
        }
        if state.blame_shown.take().is_some() {
            editor.update(cx, |e, cx| e.set_virtual_text(BLAME_SET, Vec::new(), cx));
        }
        let delay = Duration::from_millis(blame_settings.inline_blame_delay_ms);
        state.blame_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let Some(lines) = load_blame(&this, id, &editor, cx).await else {
                return;
            };
            this.update(cx, |this, cx| {
                let line = editor.read(cx).cursor_position().line;
                let Some(state) = this.editors.get_mut(&id) else {
                    return;
                };
                if this.active != Some(id) || this.blame_column {
                    return;
                }
                let Some(entry) = lines.get(line) else {
                    return;
                };
                let mut text = VirtualText::end_of_line(line, inline_text(entry, now()));
                text.color = ColorKey::new("text_disabled");
                text.italic = true;
                state.blame_shown = Some(line);
                editor.update(cx, |e, cx| e.set_virtual_text(BLAME_SET, vec![text], cx));
            })
            .ok();
        }));
    }

    /// Removes the inline blame of `id`, if shown.
    pub(crate) fn clear_inline_blame(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(state) = self.editors.get_mut(&id) else {
            return;
        };
        state.blame_task = None;
        if state.blame_shown.take().is_some()
            && let Some(editor) = state.editor.upgrade()
        {
            editor.update(cx, |e, cx| e.set_virtual_text(BLAME_SET, Vec::new(), cx));
        }
    }

    /// Shows or hides the blame column in every editor.
    pub fn toggle_blame_column(&mut self, cx: &mut Context<Self>) {
        self.blame_column = !self.blame_column;
        let ids: Vec<EntityId> = self.editors.keys().copied().collect();
        for id in ids {
            if self.blame_column {
                self.clear_inline_blame(id, cx);
                self.schedule_blame_column(id, false, cx);
            } else if let Some(state) = self.editors.get_mut(&id) {
                state.column_task = None;
                if let Some(editor) = state.editor.upgrade() {
                    editor.update(cx, |e, cx| e.clear_decorations(BLAME_COLUMN_SET, cx));
                }
            }
        }
        if !self.blame_column
            && let Some(id) = self.active
        {
            self.schedule_inline_blame(id, cx);
        }
        cx.notify();
    }

    /// Recomputes the blame column of `id`.
    pub(crate) fn schedule_blame_column(
        &mut self,
        id: EntityId,
        debounce: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.editors.get_mut(&id) else {
            return;
        };
        let Some(editor) = state.editor.upgrade() else {
            return;
        };
        if state.repo.is_none() || editor.read(cx).line_count() > MAX_COLUMN_LINES {
            return;
        }
        state.column_task = Some(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(COLUMN_DEBOUNCE).await;
            }
            let Some(lines) = load_blame(&this, id, &editor, cx).await else {
                return;
            };
            this.update(cx, |this, cx| {
                if !this.blame_column {
                    return;
                }
                let now = now();
                let count = editor.read(cx).line_count();
                let texts = (0..count)
                    .map(|line| VirtualText {
                        background: Some(ColorKey::new("hover").opacity(0.6)),
                        color: ColorKey::new("text_muted"),
                        ..VirtualText::inline(Point::new(line, 0), column_text(&lines, line, now))
                    })
                    .collect();
                editor.update(cx, |e, cx| e.set_virtual_text(BLAME_COLUMN_SET, texts, cx));
            })
            .ok();
        }));
    }
}

/// Blame for the current buffer of `id`, cached per buffer version.
async fn load_blame(
    this: &WeakEntity<GitEditors>,
    id: EntityId,
    editor: &Entity<Editor>,
    cx: &mut AsyncApp,
) -> Option<Arc<Vec<BlameLine>>> {
    let (version, rope, dirty) = editor
        .read_with(cx, |e, _| {
            (
                e.buffer().version(),
                e.buffer().text().clone(),
                e.is_dirty(),
            )
        })
        .ok()?;
    let (cached, repo, rel) = this
        .read_with(cx, |this, _| {
            let state = this.editors.get(&id)?;
            let cached = state
                .blame
                .as_ref()
                .filter(|(v, _)| *v == version)
                .map(|(_, lines)| lines.clone());
            Some((cached, state.repo.clone()?, state.rel_path()?))
        })
        .ok()??;
    if let Some(lines) = cached {
        return Some(lines);
    }
    let lines = cx
        .background_spawn(async move {
            let text = dirty.then(|| rope.to_string());
            arcode_git::blame(&repo, &rel, text.as_deref())
        })
        .await;
    let lines = match lines {
        Ok(lines) => Arc::new(lines),
        Err(err) => {
            log::debug!("blame failed: {err}");
            return None;
        }
    };
    this.update(cx, |this, _| {
        if let Some(state) = this.editors.get_mut(&id) {
            state.blame = Some((version, lines.clone()));
        }
    })
    .ok()?;
    Some(lines)
}
