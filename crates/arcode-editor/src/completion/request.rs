//! When to ask for completions, and merging the answers with the local
//! sources.

use std::rc::Rc;
use std::time::Duration;

use arcode_completion::{CompletionCandidate, path_completions};
use gpui::{AppContext, Context};

use super::menu::MenuState;
use super::session::{Mode, Pending, Session};
use super::{CompletionItem, CompletionList, CompletionProvider, CompletionTrigger, is_word_char};
use crate::Editor;

impl Editor {
    /// Sets (or removes) the provider asked for completions and signature
    /// help, e.g. the language server bridge.
    pub fn set_completion_provider(&mut self, provider: Option<Rc<dyn CompletionProvider>>) {
        self.completion.provider = provider;
    }

    /// Opens the completion list at the cursor now (Ctrl+Space).
    pub fn show_completions(&mut self, cx: &mut Context<Self>) {
        self.completion.no_results_at = None;
        self.request_completions(CompletionTrigger::Invoked, Duration::ZERO, cx);
    }

    /// Reacts to typed `text` (called after it was inserted): opens the
    /// list on word characters and trigger characters, asks again for
    /// incomplete lists, and updates signature help.
    pub(crate) fn completion_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return;
        };
        let settings = super::settings(cx);
        if settings.signature_help {
            self.signature_typed(text, cx);
        }
        let Some(cursor) = self.single_cursor() else {
            return;
        };
        let delay = Duration::from_millis(settings.delay_ms);
        let open = self.completion.session.is_some() || self.completion.pending.is_some();
        let continues = open && (is_word_char(c) || self.completion_path_open());
        if continues {
            if self
                .completion
                .session
                .as_ref()
                .is_some_and(|s| s.incomplete)
            {
                self.request_completions(CompletionTrigger::Incomplete, delay, cx);
            }
            return;
        }
        if !settings.auto_trigger {
            return;
        }
        let line = self.buffer.char_to_line(cursor);
        let line_start = self.buffer.line_start(line);
        let before = self
            .buffer
            .slice_to_string(cursor.saturating_sub(4).max(line_start)..cursor);
        let triggers = self
            .completion
            .provider
            .as_ref()
            .map(|p| p.trigger_characters(self, cx))
            .unwrap_or_default();
        if let Some(t) = triggers.into_iter().find(|t| before.ends_with(t.as_str())) {
            self.request_completions(CompletionTrigger::Character(t), delay, cx);
            return;
        }
        if matches!(c, '/' | '\\') {
            // Only a path inside a string literal (not `//` comments).
            let in_path = arcode_completion::detect_path_prefix(
                &self.buffer.line_text(line),
                cursor - line_start,
            )
            .is_some();
            if in_path {
                self.request_completions(CompletionTrigger::Invoked, delay, cx);
            }
            return;
        }
        if !is_word_char(c) {
            self.completion.no_results_at = None;
            return;
        }
        let Some(anchor) = self.completion_anchor() else {
            return;
        };
        let starts_with_digit = self
            .buffer
            .char_at(anchor.start)
            .is_some_and(|c| c.is_ascii_digit());
        if starts_with_digit || self.completion.no_results_at == Some(anchor.start) {
            return;
        }
        self.request_completions(CompletionTrigger::Invoked, delay, cx);
    }

    fn completion_path_open(&self) -> bool {
        let mode = |m: Mode| m == Mode::Path;
        self.completion
            .session
            .as_ref()
            .is_some_and(|s| mode(s.anchor.mode))
            || self
                .completion
                .pending
                .as_ref()
                .is_some_and(|p| mode(p.anchor.mode))
    }

    /// Schedules a request after `delay`; a newer one replaces it.
    pub(crate) fn request_completions(
        &mut self,
        trigger: CompletionTrigger,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        self.completion.seq += 1;
        let id = self.completion.seq;
        self.completion.request_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            this.update(cx, |editor, cx| {
                editor.fire_completion_request(id, trigger, cx)
            })
            .ok();
        }));
    }

    /// Sends request `id` to the provider and the path lister.
    fn fire_completion_request(
        &mut self,
        id: u64,
        trigger: CompletionTrigger,
        cx: &mut Context<Self>,
    ) {
        if id != self.completion.seq {
            return;
        }
        let Some(mut anchor) = self.completion_anchor() else {
            return;
        };
        // An incomplete list is asked again for the same word.
        if trigger == CompletionTrigger::Incomplete
            && let Some(session) = &self.completion.session
        {
            anchor.start = session.anchor.start;
        }
        let provider_task = match anchor.mode {
            Mode::Word => self
                .completion
                .provider
                .clone()
                .and_then(|p| p.completions(self, &trigger, cx)),
            _ => None,
        };
        let path_task = (anchor.mode == Mode::Path)
            .then(|| {
                let base = self.path()?.parent()?.to_path_buf();
                let line_start = self.buffer.line_start(anchor.line);
                let col = anchor.request_cursor - line_start;
                let (_, typed) = arcode_completion::detect_path_prefix(
                    &self.buffer.line_text(anchor.line),
                    col,
                )?;
                Some(cx.background_spawn(async move { path_completions(&base, &typed) }))
            })
            .flatten();
        if provider_task.is_none() && path_task.is_none() && anchor.mode == Mode::Path {
            return;
        }
        self.completion.pending = Some(Pending {
            id,
            anchor,
            trigger,
        });
        self.completion.request_task = Some(cx.spawn(async move |this, cx| {
            let list = match provider_task {
                Some(task) => task.await.unwrap_or_else(|err| {
                    log::debug!("completion request failed: {err:#}");
                    CompletionList::default()
                }),
                None => CompletionList::default(),
            };
            let paths = match path_task {
                Some(task) => task.await,
                None => Vec::new(),
            };
            this.update(cx, |editor, cx| {
                editor.completion_results(id, list, paths, cx)
            })
            .ok();
        }));
    }

    /// Merges the answer of request `id` with the local sources and shows
    /// it. Answers to older requests are dropped.
    pub(crate) fn completion_results(
        &mut self,
        id: u64,
        list: CompletionList,
        paths: Vec<CompletionCandidate>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.completion.pending.take_if(|p| p.id == id) else {
            return;
        };
        self.completion.request_task = None;
        let Some(query) = self.completion_query(&pending.anchor) else {
            return;
        };
        let incomplete = list.is_incomplete;
        let mut items = list.items;
        match pending.anchor.mode {
            Mode::Word => {
                // After a trigger character (`.`, `(`) only the server knows
                // what fits; words and snippets would be noise.
                let after_trigger = matches!(pending.trigger, CompletionTrigger::Character(_));
                if items.is_empty() && !after_trigger {
                    items.extend(self.buffer_word_items(&query, cx));
                }
                if super::settings(cx).snippets && !after_trigger {
                    items.extend(super::snippets::snippet_items(self.language.id, cx));
                }
            }
            Mode::Path => items.extend(paths.into_iter().map(CompletionItem::new)),
            Mode::Choice => {}
        }
        let candidates: Vec<CompletionCandidate> =
            items.iter().map(|i| i.candidate.clone()).collect();
        let menu = MenuState::rank(&mut self.completion.ranker, &query, &candidates);
        if menu.ranked.is_empty() {
            if !incomplete {
                self.completion.no_results_at = Some(pending.anchor.start);
            }
            self.hide_completions(cx);
            return;
        }
        self.completion.no_results_at = None;
        let session = Session {
            id,
            anchor: pending.anchor,
            items,
            candidates,
            incomplete,
            menu,
            query,
        };
        self.completion_open(session, cx);
    }

    /// Words of this and every other open buffer starting like `query`.
    fn buffer_word_items(&mut self, query: &str, cx: &mut Context<Self>) -> Vec<CompletionItem> {
        let needle: String = query.chars().take(1).collect();
        let current = self
            .single_cursor()
            .and_then(|c| self.word_at(self.offset_to_point(c)))
            .map(|(_, w)| w);
        let mut out = self.word_candidates(&needle, current.as_deref(), cx);
        for other in super::words::other_editors(cx.entity_id(), cx) {
            other.update(cx, |e, cx| out.extend(e.word_candidates(&needle, None, cx)));
        }
        out.into_iter().map(CompletionItem::new).collect()
    }
}
