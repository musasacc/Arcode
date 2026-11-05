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
            self.spawn_job(JobInput::Parse(self.language), lines.clone(), lines, cx);
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
                self.spawn_job(input, range.clone(), range, cx);
            }
        } else {
            syntax.reparse(self.buffer.text());
            self.highlights = None;
        }
    }

    /// Makes sure highlights cover `range`, synchronously for small buffers
    /// and via a background job for large ones. When the highlights of this
    /// version already cover part of the wanted lines, only the rest is
    /// highlighted and joined to them, so scrolling costs the lines that
    /// came into view rather than the whole view again.
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
        let keep = start..end;
        let todo = missing_lines(self.highlights.as_ref(), version, &keep);
        if todo.is_empty() {
            return;
        }
        if self.large_file {
            self.spawn_excerpt_job(todo, keep, cx);
            return;
        }
        if self.uses_async_syntax() {
            if let Some(syntax) = self.syntax.take() {
                let input = JobInput::Tree {
                    syntax,
                    reparse: false,
                };
                self.spawn_job(input, todo, keep, cx);
            }
            return;
        }
        let Some(syntax) = self.syntax.as_ref() else {
            self.highlights = None;
            return;
        };
        let spans = syntax.highlight_lines(self.buffer.text(), todo.clone());
        let cache = HighlightCache::new(version, todo, spans);
        self.highlights = Some(join_highlights(self.highlights.take(), cache, &keep));
    }

    /// Makes sure highlights cover the visible lines and, when a minimap is
    /// shown, the lines in its view, in one pass: highlights that cover only
    /// the visible lines would leave the rest of the minimap uncoloured
    /// until a second pass.
    pub(crate) fn ensure_view_highlights(
        &mut self,
        visible: Range<usize>,
        minimap: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let range = match minimap.filter(|m| !m.is_empty()) {
            Some(m) => visible.start.min(m.start)..visible.end.max(m.end),
            None => visible,
        };
        self.ensure_highlights(range, cx);
    }

    /// Lines to highlight around the last drawn view: the visible lines and
    /// the lines in the minimap's view.
    fn wanted_highlight_range(&self) -> Range<usize> {
        let view = self
            .layout
            .as_ref()
            .map_or(0..0, |layout| layout.highlighted_range());
        let start = view.start.saturating_sub(HIGHLIGHT_MARGIN);
        let end = (view.end + HIGHLIGHT_MARGIN).min(self.buffer.line_count());
        start..end
    }

    /// Highlights `range` of a large file from an excerpt parse and joins
    /// the result to the highlights kept for `keep`.
    fn spawn_excerpt_job(
        &mut self,
        range: Range<usize>,
        keep: Range<usize>,
        cx: &mut Context<Self>,
    ) {
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
            HighlightCache::new(version, range, spans)
        });
        self.syntax_task = Some(cx.spawn(async move |this, cx| {
            let cache = job.await;
            let _ = this.update(cx, |editor, cx| {
                editor.syntax_task = None;
                editor.highlights = Some(join_highlights(editor.highlights.take(), cache, &keep));
                cx.notify();
            });
        }));
    }

    /// Runs `input` in the background, highlights `range` with the result
    /// and joins that to the highlights kept for `keep`.
    fn spawn_job(
        &mut self,
        input: JobInput,
        range: Range<usize>,
        keep: Range<usize>,
        cx: &mut Context<Self>,
    ) {
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
            let cache = HighlightCache::new(version, range, spans);
            (syntax, cache)
        });
        self.syntax_task = Some(cx.spawn(async move |this, cx| {
            let (syntax, cache) = job.await;
            let _ = this.update(cx, |editor, cx| editor.finish_job(syntax, cache, &keep, cx));
        }));
    }

    fn finish_job(
        &mut self,
        syntax: Option<Syntax>,
        cache: HighlightCache,
        keep: &Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.syntax_task = None;
        let pending = std::mem::take(&mut self.pending_syntax_edits);
        let Some(mut syntax) = syntax else {
            return;
        };
        if !cache.lines.is_empty() {
            self.highlights = Some(join_highlights(self.highlights.take(), cache, keep));
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
            self.spawn_job(input, range.clone(), range, cx);
        }
        cx.notify();
    }
}

/// The part of `wanted` still to highlight, given highlights `cache` of the
/// buffer at `version`: all of it, unless the cache covers its top or its
/// bottom (or touches it there), in which case only the rest.
fn missing_lines(
    cache: Option<&HighlightCache>,
    version: u64,
    wanted: &Range<usize>,
) -> Range<usize> {
    let Some(have) = cache
        .filter(|c| c.version == version && !c.lines.is_empty())
        .map(|c| &c.lines)
    else {
        return wanted.clone();
    };
    if have.start <= wanted.start && wanted.start <= have.end {
        have.end.max(wanted.start).min(wanted.end)..wanted.end
    } else if have.start <= wanted.end && wanted.end <= have.end {
        wanted.start..have.start.max(wanted.start)
    } else {
        wanted.clone()
    }
}

/// Joins highlights `new` to `old` when both are of the same buffer version
/// and their lines touch, keeping the lines of `keep` (and all of `new`);
/// otherwise `new` replaces `old`.
fn join_highlights(
    old: Option<HighlightCache>,
    new: HighlightCache,
    keep: &Range<usize>,
) -> HighlightCache {
    let Some(mut old) = old.filter(|o| {
        o.version == new.version && o.lines.start <= new.lines.end && new.lines.start <= o.lines.end
    }) else {
        return new;
    };
    let start = old.lines.start.max(keep.start).min(new.lines.start);
    let end = old.lines.end.min(keep.end).max(new.lines.end);
    if start == new.lines.start && end == new.lines.end {
        return new;
    }
    let mut new_spans = new.spans;
    let spans = (start..end)
        .map(|line| {
            let (lines, spans) = if new.lines.contains(&line) {
                (&new.lines, &mut new_spans)
            } else {
                (&old.lines, &mut old.spans)
            };
            spans
                .get_mut(line - lines.start)
                .map(std::mem::take)
                .unwrap_or_default()
        })
        .collect();
    HighlightCache::new(new.version, start..end, spans)
}

#[cfg(test)]
mod tests {
    use arcode_syntax::HighlightSpan;

    use super::*;

    /// A cache whose line `l` holds one span named after it.
    fn cache(version: u64, lines: Range<usize>) -> HighlightCache {
        const NAMES: [&str; 4] = ["a", "b", "c", "d"];
        let spans = lines
            .clone()
            .map(|l| {
                vec![HighlightSpan {
                    range: l..l + 1,
                    name: NAMES[l % 4],
                }]
            })
            .collect();
        HighlightCache::new(version, lines, spans)
    }

    #[test]
    fn only_lines_not_yet_highlighted_are_asked_for() {
        let c = cache(3, 100..200);
        // Scrolling down: only the lines below the cache.
        assert_eq!(missing_lines(Some(&c), 3, &(150..260)), 200..260);
        // Scrolling up: only the lines above it.
        assert_eq!(missing_lines(Some(&c), 3, &(40..160)), 40..100);
        // Touching counts as next to it.
        assert_eq!(missing_lines(Some(&c), 3, &(200..230)), 200..230);
        // Far away, around it on both sides, another version or none: all.
        assert_eq!(missing_lines(Some(&c), 3, &(500..600)), 500..600);
        assert_eq!(missing_lines(Some(&c), 3, &(50..250)), 50..250);
        assert_eq!(missing_lines(Some(&c), 4, &(150..260)), 150..260);
        assert_eq!(missing_lines(None, 3, &(150..260)), 150..260);
        // Already covered.
        assert!(missing_lines(Some(&c), 3, &(120..180)).is_empty());
    }

    #[test]
    fn joined_highlights_keep_every_line_in_its_place() {
        let joined = join_highlights(Some(cache(3, 100..200)), cache(3, 200..260), &(150..260));
        assert_eq!(joined.lines, 150..260);
        assert_eq!(joined.spans.len(), 110);
        for (ix, spans) in joined.spans.iter().enumerate() {
            let line = 150 + ix;
            assert_eq!(spans[0].range, line..line + 1, "line {line}");
        }

        let up = join_highlights(Some(cache(3, 100..200)), cache(3, 40..100), &(40..160));
        assert_eq!(up.lines, 40..160);
        assert_eq!(up.spans[70][0].range, 110..111);

        // Lines the view no longer needs are dropped, those of `new` never.
        let old = cache(3, 0..300);
        let joined = join_highlights(Some(old), cache(3, 290..400), &(350..400));
        assert_eq!(joined.lines, 290..400);

        // Another version, or lines that do not touch: replaced.
        let joined = join_highlights(Some(cache(2, 100..200)), cache(3, 200..260), &(150..260));
        assert_eq!(joined.lines, 200..260);
        let joined = join_highlights(Some(cache(3, 100..200)), cache(3, 300..360), &(300..360));
        assert_eq!(joined.lines, 300..360);
        // A join is a new cache, so views built from the old one rebuild.
        let old = cache(3, 100..200);
        let old_id = old.id;
        let joined = join_highlights(Some(old), cache(3, 200..260), &(150..260));
        assert_ne!(joined.id, old_id);
    }
}
