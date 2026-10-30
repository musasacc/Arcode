//! Accepting an item: the main edit (plain text or snippet), additional
//! edits such as auto-imports in the same undo step, resolving them late
//! when the provider had not sent them yet, and follow-up commands.

use std::ops::Range;

use arcode_buffer::{Selection, SelectionSet, UndoGroup};
use arcode_completion::CandidateKind;
use gpui::Context;

use super::CompletionItem;
use super::session::{Anchor, Mode};
use crate::Editor;
use crate::api::map_offset;

/// Commands from language servers the editor runs itself.
const TRIGGER_SIGNATURE: &str = "editor.action.triggerParameterHints";
const TRIGGER_SUGGEST: &str = "editor.action.triggerSuggest";

/// `\r\n` and `\r` as `\n`, like the buffer stores them.
fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl Editor {
    /// Accepts the selected completion (Enter). Returns whether one was
    /// shown.
    pub fn confirm_completion(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selected) = self.completion.session.as_ref().map(|s| s.menu.selected) else {
            return false;
        };
        self.completion_confirm_at(selected, false, cx)
    }

    /// Accepts row `row` of the list; `replace` also replaces the rest of
    /// the word after the cursor.
    pub(crate) fn completion_confirm_at(
        &mut self,
        row: usize,
        replace: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        // An empty list (waiting for a re-request) accepts nothing, and a
        // read-only buffer takes no edits.
        if self.read_only
            || self
                .completion
            .session
            .as_ref()
            .is_none_or(|s| row >= s.menu.ranked.len())
        {
            return false;
        }
        let Some(session) = self.completion.session.take() else {
            return false;
        };
        self.hide_completions(cx);
        let Some(ranked) = session.menu.ranked.get(row) else {
            return false;
        };
        let item = session.items[ranked.index].clone();
        let Some(range) = self.completion_range(&item, &session.anchor, replace) else {
            return false;
        };
        let main_line = self.buffer.char_to_line(range.start);
        let extra = self.extra_edit_offsets(&item, &range);
        let version = self.apply_completion(range, &item, extra, cx);
        if item.needs_resolve && item.additional_edits.is_empty() {
            self.resolve_after_accept(item.clone(), version, main_line, cx);
        }
        self.run_completion_command(item.command.as_deref(), cx);
        if item.candidate.kind == CandidateKind::Folder && session.anchor.mode == Mode::Path {
            // Go on into the folder.
            self.show_completions(cx);
        }
        true
    }

    /// The char range accepting `item` replaces now. Text typed at the
    /// cursor since the list arrived is included in its edit.
    fn completion_range(
        &self,
        item: &CompletionItem,
        anchor: &Anchor,
        replace: bool,
    ) -> Option<Range<usize>> {
        let sel = self.selections.primary();
        if anchor.mode == Mode::Choice {
            return Some(sel.range());
        }
        let cursor = self.single_cursor()?;
        let Some(edit) = &item.edit else {
            let end = if replace {
                self.word_end_from(cursor)
            } else {
                cursor
            };
            return Some(anchor.start.min(cursor)..end);
        };
        let r = if replace { &edit.replace } else { &edit.insert };
        let start = self.point_to_offset(r.start).min(cursor);
        let mut end = self.point_to_offset(r.end);
        if end >= anchor.request_cursor {
            let delta = cursor as isize - anchor.request_cursor as isize;
            end = ((end as isize + delta).max(cursor as isize)) as usize;
        }
        Some(start..end.max(start))
    }

    /// Additional edits of `item` as char ranges, without any overlapping
    /// the main edit `main`.
    fn extra_edit_offsets(
        &self,
        item: &CompletionItem,
        main: &Range<usize>,
    ) -> Vec<(Range<usize>, String)> {
        let mut out: Vec<(Range<usize>, String)> = Vec::new();
        for (r, text) in &item.additional_edits {
            let a = self.point_to_offset(r.start);
            let b = self.point_to_offset(r.end).max(a);
            let overlaps = |x: &Range<usize>| a < x.end && x.start < b || (a == b && a == x.start);
            if overlaps(main) || out.iter().any(|(o, _)| overlaps(o)) {
                log::debug!("dropping an overlapping additional completion edit");
                continue;
            }
            out.push((a..b, normalize_breaks(text)));
        }
        out
    }

    /// Applies the main edit and `extra` as one undo step; returns the
    /// buffer version afterwards.
    fn apply_completion(
        &mut self,
        range: Range<usize>,
        item: &CompletionItem,
        extra: Vec<(Range<usize>, String)>,
        cx: &mut Context<Self>,
    ) -> u64 {
        if self.read_only {
            return self.buffer.version();
        }
        // The buffer stores LF only; place the cursor by what it inserts.
        let text = normalize_breaks(item.candidate.insert.text());
        if item.candidate.insert.is_snippet() {
            self.insert_snippet_at(range, &text, extra, cx);
            return self.buffer.version();
        }
        let len = text.chars().count();
        let mut raw = extra;
        raw.push((range.clone(), text));
        raw.sort_by_key(|(r, _)| (r.start, r.end));
        let shifts: Vec<(Range<usize>, usize)> = raw
            .iter()
            .map(|(r, t)| (r.clone(), t.chars().count()))
            .collect();
        // The main edit itself starts at `range.start`, so only earlier ones
        // shift it.
        let start = map_offset(range.start, &shifts);
        let after = SelectionSet::from_selection(Selection::cursor(start + len));
        let before = self.selections.clone();
        self.buffer.break_undo_group();
        let edits = self
            .buffer
            .edit_with_selections(raw, UndoGroup::New, &before, &after);
        self.buffer.break_undo_group();
        self.selections = after;
        self.after_edits(edits, cx);
        self.buffer.version()
    }

    /// Resolves an accepted item for its additional edits (auto-imports)
    /// and applies them when they arrive, as long as they lie above the
    /// line it was inserted on and nothing else changed since.
    fn resolve_after_accept(
        &mut self,
        item: CompletionItem,
        version: u64,
        line: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.completion.provider.clone() else {
            return;
        };
        let Some(task) = provider.resolve(self, &item, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let Ok(resolved) = task.await else {
                return;
            };
            this.update(cx, |editor, cx| {
                let edits = resolved.additional_edits;
                let above = edits.iter().all(|(r, _)| r.end.line < line);
                if edits.is_empty() || (editor.buffer.version() != version && !above) {
                    return;
                }
                editor.apply_edits(edits, cx);
            })
            .ok();
        })
        .detach();
    }

    fn run_completion_command(&mut self, command: Option<&str>, cx: &mut Context<Self>) {
        match command {
            Some(TRIGGER_SIGNATURE) => {
                if super::settings(cx).signature_help {
                    self.show_signature_help(cx);
                }
            }
            Some(TRIGGER_SUGGEST) => self.show_completions(cx),
            Some(other) => log::debug!("completion command `{other}` is not supported"),
            None => {}
        }
    }
}
