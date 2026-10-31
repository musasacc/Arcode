//! The file finder (Ctrl+P): a modal with a query field and the project's
//! files ranked fuzzily. Ranking runs on a background thread.

use std::path::PathBuf;
use std::sync::Arc;

use arcode_ui_kit::{IconName, theme, truncated};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, ScrollStrategy,
    Subscription, Task, UniformListScrollHandle, Window, actions, prelude::*, uniform_list,
};

use crate::file_icons::file_icon;
use crate::fuzzy::{FileMatch, ranges_within, rank};
use crate::picker_frame::{
    PALETTE_W, count_status, empty_state, footer, input_row, match_style, picker_frame,
    picker_list, picker_row, row_detail, row_icon, row_label, scroll_strategy,
};
use crate::text_field::{TextChanged, TextField};

actions!(finder, [SelectPrev, SelectNext, Confirm, Dismiss]);

/// Key context of the finder (also used by the command palette).
pub(crate) const CONTEXT: &str = "Finder";
const MAX_RESULTS: usize = 200;
const MAX_VISIBLE_ROWS: usize = 10;

/// Registers the finder's key bindings.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("enter", Confirm, ctx),
        KeyBinding::new("escape", Dismiss, ctx),
    ]);
}

/// Events emitted by the [`Finder`].
pub enum FinderEvent {
    /// The user chose a file.
    Open(PathBuf),
    /// The finder should close.
    Dismissed,
    /// The query starts with `>`: switch to the command palette with the
    /// rest of it.
    SwitchToCommands(String),
}

/// The finder modal.
pub struct Finder {
    root: PathBuf,
    input: Entity<TextField>,
    files: Arc<Vec<String>>,
    loading: bool,
    matches: Vec<FileMatch>,
    selected: usize,
    scroll: UniformListScrollHandle,
    match_task: Option<Task<()>>,
    /// Enter was pressed while ranking for the latest query was still
    /// running: open the top match once it is done.
    confirm_pending: bool,
    _subscription: Subscription,
}

impl EventEmitter<FinderEvent> for Finder {}

impl Focusable for Finder {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Finder {
    /// Creates a finder over `files` (relative paths under `root`).
    /// `loading` shows an indexing hint until [`Finder::set_files`].
    pub fn new(
        root: PathBuf,
        files: Arc<Vec<String>>,
        loading: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextField::new("Search files by name", cx));
        let sub = cx.subscribe(&input, |finder, _, _: &TextChanged, cx| {
            finder.update_matches(cx)
        });
        let mut finder = Self {
            root,
            input,
            files,
            loading,
            matches: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            match_task: None,
            confirm_pending: false,
            _subscription: sub,
        };
        finder.update_matches(cx);
        finder
    }

    /// Replaces the candidate list (e.g. after a fresh project scan).
    pub fn set_files(&mut self, files: Arc<Vec<String>>, cx: &mut Context<Self>) {
        self.files = files;
        self.loading = false;
        self.update_matches(cx);
    }

    /// Replaces the query text.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |f, cx| f.set_text(query, cx));
    }

    fn update_matches(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        if let Some(rest) = query.strip_prefix('>') {
            cx.emit(FinderEvent::SwitchToCommands(rest.to_owned()));
            return;
        }
        let files = self.files.clone();
        let work = cx.background_spawn(async move { rank(&files, &query, MAX_RESULTS) });
        self.match_task = Some(cx.spawn(async move |this, cx| {
            let matches = work.await;
            this.update(cx, |finder, cx| {
                finder.matches = matches;
                finder.selected = 0;
                finder.scroll.scroll_to_item(0, ScrollStrategy::Top);
                finder.match_task = None;
                if std::mem::take(&mut finder.confirm_pending) {
                    finder.confirm_index(0, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            let from = self.selected;
            self.selected = ix.min(self.matches.len() - 1);
            self.scroll
                .scroll_to_item(self.selected, scroll_strategy(from, self.selected));
            cx.notify();
        }
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let ix = if self.selected == 0 {
            self.matches.len().saturating_sub(1)
        } else {
            self.selected - 1
        };
        self.select(ix, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let ix = if self.selected + 1 >= self.matches.len() {
            0
        } else {
            self.selected + 1
        };
        self.select(ix, cx);
    }

    fn confirm_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.matches.get(ix)
            && let Some(rel) = self.files.get(m.index)
        {
            cx.emit(FinderEvent::Open(self.root.join(rel)));
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        // The shown matches belong to an older query until ranking ends.
        if self.match_task.is_some() {
            self.confirm_pending = true;
            return;
        }
        self.confirm_index(self.selected, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FinderEvent::Dismissed);
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let m = &self.matches[ix];
        let path = self.files.get(m.index).map(String::as_str).unwrap_or("");
        let split = path.rfind('/').map_or(0, |i| i + 1);
        let (dir, name) = path.split_at(split);
        let style = match_style(cx);
        let highlight = |offset: usize, len: usize| {
            ranges_within(&m.highlights, offset, len)
                .into_iter()
                .map(|r| (r, style))
                .collect::<Vec<_>>()
        };
        let name_text = truncated(name.to_string()).highlights(highlight(split, name.len()));
        let dir_trimmed = dir.trim_end_matches('/');
        // A cut folder keeps its end, next to the file.
        let dir_text = truncated(dir_trimmed.to_string())
            .start()
            .highlights(highlight(0, dir_trimmed.len()));
        let (icon_name, icon_color) = file_icon(name, &c);
        let selected = ix == self.selected;
        picker_row(ix, selected, cx)
            .child(row_icon(icon_name, selected, Some(icon_color), cx))
            .child(row_label(name_text, !dir_trimmed.is_empty()))
            .child(row_detail(dir_text, cx))
            .on_click(cx.listener(move |finder, _, _, cx| finder.confirm_index(ix, cx)))
            .into_any_element()
    }
}

impl Render for Finder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.matches.len();
        let indexing = self.loading && self.files.is_empty();
        let body = if indexing {
            empty_state("Indexing project files…", cx).into_any_element()
        } else if count == 0 {
            empty_state("No matching files", cx).into_any_element()
        } else {
            let list = uniform_list(
                "finder-results",
                count,
                cx.processor(|finder, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|ix| finder.render_row(ix, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone());
            picker_list(list, count, MAX_VISIBLE_ROWS).into_any_element()
        };
        let status = (!indexing).then(|| count_status(None, count, self.files.len()));
        picker_frame(PALETTE_W, cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .child(input_row(
                IconName::Search,
                None,
                self.input.clone(),
                Some("Files".into()),
                cx,
            ))
            .child(body)
            .child(footer("open", status, cx))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn enter_right_after_typing_opens_the_match_for_the_new_query(cx: &mut TestAppContext) {
        crate::test_support::init(cx);
        let files = Arc::new(vec![".gitignore".to_string(), "src/main.rs".to_string()]);
        let (finder, cx) =
            cx.add_window_view(move |_, cx| Finder::new(PathBuf::from("/p"), files, false, cx));
        cx.run_until_parked();
        let opened = Rc::new(RefCell::new(Vec::new()));
        let sink = opened.clone();
        cx.update(|_, cx| {
            cx.subscribe(&finder, move |_, e: &FinderEvent, _| {
                if let FinderEvent::Open(path) = e {
                    sink.borrow_mut().push(path.clone());
                }
            })
            .detach()
        });
        // Enter before ranking for "main" has finished.
        finder.update(cx, |f, cx| f.set_query("main", cx));
        finder.update_in(cx, |f, window, cx| f.confirm(&Confirm, window, cx));
        assert!(opened.borrow().is_empty());
        cx.run_until_parked();
        assert_eq!(*opened.borrow(), [PathBuf::from("/p/src/main.rs")]);
    }
}
