//! Keeping the syntax tree and highlights in sync with the buffer.
//!
//! Small buffers are parsed, reparsed and highlighted synchronously. For
//! large buffers every tree-sitter call (initial parse, reparse, viewport
//! highlight queries) runs as a job on the background executor, because
//! error recovery on huge files can take seconds. While a job runs the tree
//! is checked out: edits are queued and applied when it returns, and the
//! last highlights stay on screen.
//!
//! Files above the `editor.large_file_bytes` / `editor.large_file_lines`
//! cutoffs keep no tree at all: each viewport is highlighted from a parse
//! of a small excerpt around it ([`arcode_syntax::highlight_excerpt`]), so
//! memory stays close to the plain-text case.

use std::ops::Range;

use arcode_buffer::Edit;
use arcode_syntax::{InputEditData, Language, Syntax};
use gpui::{AppContext as _, Context};

use crate::Editor;
use crate::editor::HighlightCache;

/// Buffers at least this large (bytes) use background syntax jobs.
pub(crate) const ASYNC_SYNTAX_BYTES: usize = 256 * 1024;
/// Extra lines highlighted around the viewport so small scrolls reuse them.
pub(crate) const HIGHLIGHT_MARGIN: usize = 32;

/// Converts a buffer edit to tree-sitter's input edit.
pub(crate) fn input_edit(e: &Edit) -> InputEditData {
    InputEditData {
        start_byte: e.start_byte,
        old_end_byte: e.old_end_byte,
        new_end_byte: e.new_end_byte,
        start_position: (e.start_position.row, e.start_position.column),
        old_end_position: (e.old_end_position.row, e.old_end_position.column),
        new_end_position: (e.new_end_position.row, e.new_end_position.column),
    }
}

/// What a background job starts from.
enum JobInput {
    /// Parse from scratch.
    Parse(&'static Language),
    /// Use this tree; reparse it first if `reparse`.
    Tree { syntax: Syntax, reparse: bool },
}

impl Editor {
    /// Whether a background syntax job holds the tree.
    pub(crate) fn syntax_job_running(&self) -> bool {
        self.syntax_task.is_some()
    }

    fn uses_async_syntax(&self) -> bool {
        self.buffer.len_bytes() >= ASYNC_SYNTAX_BYTES
    }

    /// Whether the buffer is above the large-file cutoffs in settings.
    fn exceeds_large_file_cutoff(&self, cx: &Context<Self>) -> bool {
        let ed = &arcode_ui_kit::settings(cx).editor;
        self.buffer.len_bytes() as u64 >= ed.large_file_bytes.max(1)
            || self.buffer.line_count() >= ed.large_file_lines.max(1)
    }

    /// Whether the editor treats this buffer as a large file (no tree).
    pub fn is_large_file(&self) -> bool {
        self.large_file
    }

    /// Creates the syntax tree, in the background for large buffers.
    pub(crate) fn init_syntax(&mut self, cx: &mut Context<Self>) {
        if !self.language.has_grammar() {
            return;
        }
        self.large_file = self.exceeds_large_file_cutoff(cx);
        if self.large_file {
            log::info!(
                "{}: large file, highlighting from viewport excerpts",
                self.title()
            );
            return;
        }
        if self.uses_async_syntax() {
            let lines = 0..self.buffer.line_count().min(100);
            self.spawn_job(JobInput::Parse(self.language), lines, cx);
        } else {
            self.syntax = Syntax::new(self.language, self.buffer.text());
        }
    }

    /// Feeds `edits` to the syntax tree and reparses.
    pub(crate) fn sync_syntax(&mut self, edits: &[Edit], cx: &mut Context<Self>) {
        if self.large_file {
            // The excerpt cache is keyed by version; the next layout
            // requests a fresh excerpt.
            return;
        }
        if self.syntax_job_running() {
            self.pending_syntax_edits
                .extend(edits.iter().map(input_edit));
            return;
        }
        let is_async = self.uses_async_syntax();
        let Some(syntax) = self.syntax.as_mut() else {
            return;
        };
        for e in edits {
            syntax.edit(&input_edit(e));
        }
        if is_async {
            let range = self.wanted_highlight_range();
            if let Some(syntax) = self.syntax.take() {
                let input = JobInput::Tree {
                    syntax,
                    reparse: true,
                };
                self.spawn_job(input, range, cx);
            }
        } else {
            syntax.reparse(self.buffer.text());
            self.highlights = None;
        }
    }

    /// Makes sure highlights cover `range` (visible lines), synchronously for
    /// small buffers and via a background job for large ones.
    pub(crate) fn ensure_highlights(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let version = self.buffer.version();
        let covered = self.highlights.as_ref().is_some_and(|c| {
            c.version == version && c.lines.start <= range.start && range.end <= c.lines.end
        });
        if covered || self.syntax_job_running() {
            return;
        }
        let start = range.start.saturating_sub(HIGHLIGHT_MARGIN);
        let end = (range.end + HIGHLIGHT_MARGIN).min(self.buffer.line_count());
        if self.large_file {
            self.spawn_excerpt_job(start..end, cx);
            return;
        }
        if self.uses_async_syntax() {
            if let Some(syntax) = self.syntax.take() {
                let input = JobInput::Tree {
                    syntax,
                    reparse: false,
                };
                self.spawn_job(input, start..end, cx);
            }
            return;
        }
        let Some(syntax) = self.syntax.as_ref() else {
            self.highlights = None;
            return;
        };
        let spans = syntax.highlight_lines(self.buffer.text(), start..end);
        self.highlights = Some(HighlightCache {
            version,
            lines: start..end,
            spans,
        });
    }

    /// Lines to highlight around the last visible range.
    fn wanted_highlight_range(&self) -> Range<usize> {
        let visible = self.visible_line_range();
        let start = visible.start.saturating_sub(HIGHLIGHT_MARGIN);
        let end = (visible.end + HIGHLIGHT_MARGIN).min(self.buffer.line_count());
        start..end
    }

    /// Highlights `range` of a large file from an excerpt parse.
    fn spawn_excerpt_job(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let rope = self.buffer.text().clone();
        let version = self.buffer.version();
        let language = self.language;
        let job = cx.background_spawn(async move {
            let started = std::time::Instant::now();
            let spans = arcode_syntax::highlight_excerpt(language, &rope, range.clone());
            log::trace!(
                "excerpt highlight of {range:?} took {:?}",
                started.elapsed()
            );
            HighlightCache {
                version,
                lines: range,
                spans,
            }
        });
        self.syntax_task = Some(cx.spawn(async move |this, cx| {
            let cache = job.await;
            let _ = this.update(cx, |editor, cx| {
                editor.syntax_task = None;
                editor.highlights = Some(cache);
                cx.notify();
            });
        }));
    }

    fn spawn_job(&mut self, input: JobInput, range: Range<usize>, cx: &mut Context<Self>) {
        let rope = self.buffer.text().clone();
        let version = self.buffer.version();
        let job = cx.background_spawn(async move {
            let started = std::time::Instant::now();
            let syntax = match input {
                JobInput::Parse(language) => Syntax::new(language, &rope),
                JobInput::Tree {
                    mut syntax,
                    reparse,
                } => {
                    if reparse {
                        syntax.reparse(&rope);
                    }
                    Some(syntax)
                }
            };
            let parsed = started.elapsed();
            let spans = syntax
                .as_ref()
                .map(|s| s.highlight_lines(&rope, range.clone()))
                .unwrap_or_default();
            log::trace!(
                "syntax job: parse {parsed:?}, highlight {:?}",
                started.elapsed() - parsed
            );
            let cache = HighlightCache {
                version,
                lines: range,
                spans,
            };
            (syntax, cache)
        });
        self.syntax_task = Some(cx.spawn(async move |this, cx| {
            let (syntax, cache) = job.await;
            let _ = this.update(cx, |editor, cx| editor.finish_job(syntax, cache, cx));
        }));
    }

    fn finish_job(
        &mut self,
        syntax: Option<Syntax>,
        cache: HighlightCache,
        cx: &mut Context<Self>,
    ) {
        self.syntax_task = None;
        let pending = std::mem::take(&mut self.pending_syntax_edits);
        let Some(mut syntax) = syntax else {
            return;
        };
        if !cache.lines.is_empty() {
            self.highlights = Some(cache);
        }
        if pending.is_empty() {
            self.syntax = Some(syntax);
        } else {
            for e in &pending {
                syntax.edit(e);
            }
            let range = self.wanted_highlight_range();
            let input = JobInput::Tree {
                syntax,
                reparse: true,
            };
            self.spawn_job(input, range, cx);
        }
        cx.notify();
    }
}
