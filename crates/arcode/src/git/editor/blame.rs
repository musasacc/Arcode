//! Inline blame for the cursor line and the blame column.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arcode_buffer::{LineEnding, Point};
use arcode_editor::{ColorKey, Editor, EditorStyle, VirtualText};
use arcode_git::BlameLine;
use arcode_ui_kit::{glyph_advance, settings};
use gpui::{App, AppContext, AsyncApp, Context, Entity, EntityId, SharedString, WeakEntity};

use super::{BLAME_COLUMN_SET, BLAME_SET, GitEditors};
use crate::git::blame_text::{column_text_with, inline_text, now, unicode_cells};

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
                // Italic faces often have no fallback for CJK and other
                // scripts (they render as boxes): only slant Latin-ish text.
                text.italic = italic_safe(&text.text);
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
                let cells = glyph_cells(cx);
                let texts = (0..count)
                    .map(|line| VirtualText {
                        background: Some(ColorKey::new("hover").opacity(0.6)),
                        color: ColorKey::new("text_muted"),
                        ..VirtualText::inline(
                            Point::new(line, 0),
                            column_text_with(&lines, line, now, &cells),
                        )
                    })
                    .collect();
                editor.update(cx, |e, cx| e.set_virtual_text(BLAME_COLUMN_SET, texts, cx));
            })
            .ok();
        }));
    }
}

/// The width of a char in cells of the editor font, from the glyph the
/// editor will draw (fallback fonts included), so the blame column pads the
/// code to the same x on every line. ASCII is one cell; a wide char with no
/// known font is taken as one em, as CJK fonts draw ideographs.
fn glyph_cells(cx: &App) -> impl Fn(char) -> f32 + use<> {
    let style = EditorStyle::from_settings("", cx);
    let fallbacks: Vec<SharedString> = settings(cx)
        .editor
        .font_fallbacks
        .iter()
        .map(|f| SharedString::from(f.clone()))
        .collect();
    let ts = cx.text_system().clone();
    let size = style.font_size;
    let cell = glyph_advance(&style.font, &[], 'm', size, &ts).unwrap_or(size * 0.6);
    let known = RefCell::new(HashMap::new());
    move |c: char| {
        if c.is_ascii() {
            return 1.0;
        }
        *known.borrow_mut().entry(c).or_insert_with(|| {
            match glyph_advance(&style.font, &fallbacks, c, size, &ts) {
                Some(advance) => advance / cell,
                None if unicode_cells(c) >= 2.0 => size / cell,
                None => unicode_cells(c),
            }
        })
    }
}

/// Blame for the current buffer of `id`, cached per buffer version.
async fn load_blame(
    this: &WeakEntity<GitEditors>,
    id: EntityId,
    editor: &Entity<Editor>,
    cx: &mut AsyncApp,
) -> Option<Arc<Vec<BlameLine>>> {
    let (version, rope, dirty, crlf) = editor
        .read_with(cx, |e, _| {
            (
                e.buffer().version(),
                e.buffer().text().clone(),
                e.is_dirty(),
                e.buffer().line_ending() == LineEnding::CrLf,
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
            // The rope holds `\n` breaks; git compares against the file as
            // committed, so give it the buffer's own line ending (as Stage
            // hunk does) or every line of a CRLF file reads as changed.
            let text = dirty.then(|| {
                let text = rope.to_string();
                if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text
                }
            });
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

/// Whether `text` can be drawn italic: only Latin, Greek and Cyrillic
/// letters and common punctuation, which the editor font's italic face
/// covers. Other scripts would fall back to fonts without an italic face.
fn italic_safe(text: &str) -> bool {
    text.chars()
        .all(|c| (c as u32) < 0x0530 || ('\u{2000}'..='\u{206F}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::italic_safe;

    #[test]
    fn italic_only_for_scripts_the_italic_face_covers() {
        assert!(italic_safe("Zoë Müller, 3 days ago • Fix the parser…"));
        assert!(italic_safe("Иван, 2 hours ago • Исправить"));
        assert!(!italic_safe("Zhang Wei 张伟, 1 day ago • 初始 commit"));
        assert!(!italic_safe("Ada, 1 day ago • 🎉 release"));
    }
}
