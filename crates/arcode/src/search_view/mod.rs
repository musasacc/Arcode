//! The Search view (activity bar search icon, Ctrl+Shift+F): project-wide
//! search with `arcode_search` on the background executor. Results stream
//! in grouped by file while the search runs; typing re-runs it after
//! `search.debounce_ms` and cancels the superseded run. Replace in a file
//! or in every file goes through the editor for open files (undoable) and
//! writes other files directly (see [`replace`]).

mod buffers;
pub mod line_row;
mod nav;
mod render;
pub mod replace;
pub mod results;
#[cfg(test)]
mod review_search_view;
#[cfg(test)]
mod tests;

use std::ops::{ControlFlow, Range};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::Editor;
use arcode_search::{CancelToken, FileMatches, SearchQuery, SearchStats, parse_glob_list};
use arcode_ui_kit::settings;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, UniformListScrollHandle, Window,
};

use crate::search_widgets::{SearchOption, SearchOptions, short_error};
use crate::text_field::{TextChanged, TextField};
pub use nav::{ReplaceAllMatches, RunSearch, bind_keys};
use results::{Results, Row};

/// Key context of the Search view.
pub const CONTEXT: &str = "ProjectSearch";
/// Key context of the replace field.
pub const REPLACE_CONTEXT: &str = "ProjectSearchReplace";
/// Key context of the include and exclude glob fields.
pub const GLOB_CONTEXT: &str = "ProjectSearchGlob";
/// How long streamed results are collected before the list redraws.
const BATCH_DELAY: Duration = Duration::from_millis(40);

/// Lists the open editors (for replacing in open buffers).
pub type EditorsFn = Rc<dyn Fn(&App) -> Vec<Entity<Editor>>>;

/// Events emitted by the Search view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectSearchEvent {
    /// Open `path` and select `range`.
    Open {
        /// The file.
        path: PathBuf,
        /// The match.
        range: Range<Point>,
    },
    /// A message for a toast.
    Message(String),
}

/// Where the last search stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// No query.
    Idle,
    /// Running; results keep arriving.
    Searching,
    /// Finished (or stopped at the limit).
    Done(SearchStats),
    /// The query or a glob is invalid.
    Invalid(SharedString),
}

/// The Search view.
pub struct ProjectSearch {
    root: Option<PathBuf>,
    query: Entity<TextField>,
    replacement: Entity<TextField>,
    include: Entity<TextField>,
    exclude: Entity<TextField>,
    options: SearchOptions,
    show_replace: bool,
    show_globs: bool,
    results: Results,
    rows: Vec<Row>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    status: Status,
    /// Result limit of the current query ("show more" raises it).
    limit: usize,
    last_query: Option<SearchQuery>,
    cancel: Option<CancelToken>,
    task: Option<Task<()>>,
    debounce: Option<Task<()>>,
    /// Outcome of the last replace.
    notice: Option<SharedString>,
    replacing: bool,
    /// The replace preview's compiled query, for the query and replacement
    /// it was built from (rebuilt only when they change).
    preview: Option<line_row::Preview>,
    editors: Option<EditorsFn>,
    focus_handle: FocusHandle,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ProjectSearchEvent> for ProjectSearch {}

impl Focusable for ProjectSearch {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ProjectSearch {
    /// Creates the view (no project).
    pub fn new(cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| TextField::new(placeholder, cx))
        };
        let query = field("Search", cx);
        let replacement = field("Replace", cx);
        let include = field("files to include (e.g. src, *.rs)", cx);
        let exclude = field("files to exclude", cx);
        let mut subs = Vec::new();
        for input in [&query, &include, &exclude] {
            subs.push(cx.subscribe(input, |view, _, _: &TextChanged, cx| {
                view.limit = settings(cx).search.max_results;
                view.schedule(cx);
            }));
        }
        subs.push(cx.subscribe(&replacement, |_, _, _: &TextChanged, cx| cx.notify()));
        Self {
            root: None,
            query,
            replacement,
            include,
            exclude,
            options: SearchOptions::default(),
            show_replace: false,
            show_globs: false,
            results: Results::default(),
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            status: Status::Idle,
            limit: settings(cx).search.max_results,
            last_query: None,
            cancel: None,
            task: None,
            debounce: None,
            notice: None,
            replacing: false,
            preview: None,
            editors: None,
            focus_handle: cx.focus_handle(),
            _subs: subs,
        }
    }

    /// Sets how open editors are found (for replacing in open buffers).
    pub fn set_editors_fn(&mut self, editors: EditorsFn) {
        self.editors = Some(editors);
    }

    /// Sets the project root; clears the results.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.stop();
        self.last_query = None;
        self.results.clear();
        self.notice = None;
        self.rebuild_rows();
        self.status = Status::Idle;
        if self.root.is_some() {
            self.run(true, cx);
        }
        cx.notify();
    }

    /// Focuses the query field (selecting its text) and optionally shows
    /// the replace field; a one-line `seed` replaces the query.
    pub fn focus_query(
        &mut self,
        seed: Option<String>,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if replace {
            self.show_replace = true;
        }
        if let Some(seed) = seed.filter(|s| !s.is_empty() && !s.contains('\n')) {
            self.query.update(cx, |f, cx| f.set_text(&seed, cx));
        }
        self.query.update(cx, |f, cx| f.select_all_text(cx));
        window.focus(&self.query.focus_handle(cx));
        cx.notify();
    }

    /// Sets the query text (tests and seeding).
    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Sets the replacement text.
    pub fn set_replacement(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replacement.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Sets the include and exclude glob fields.
    pub fn set_globs(&mut self, include: &str, exclude: &str, cx: &mut Context<Self>) {
        self.include.update(cx, |f, cx| f.set_text(include, cx));
        self.exclude.update(cx, |f, cx| f.set_text(exclude, cx));
    }

    /// The results so far.
    pub fn results(&self) -> &Results {
        &self.results
    }

    /// The search status.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The last replace outcome.
    pub fn notice(&self) -> Option<&SharedString> {
        self.notice.as_ref()
    }

    /// Flips a search option and searches again.
    pub fn toggle_option(&mut self, option: SearchOption, cx: &mut Context<Self>) {
        option.toggle(&mut self.options);
        self.run(false, cx);
    }

    /// The query as it would run now.
    pub fn build_query(&self, cx: &App) -> SearchQuery {
        let s = &settings(cx).search;
        let mut exclude = parse_glob_list(self.exclude.read(cx).text());
        exclude.extend(s.exclude.iter().cloned());
        exclude.extend(crate::project::skipped_dir_globs());
        SearchQuery {
            include_globs: parse_glob_list(self.include.read(cx).text()),
            exclude_globs: exclude,
            include_hidden: s.include_hidden,
            respect_gitignore: s.respect_gitignore,
            max_results: self.limit,
            ..self.options.query(self.query.read(cx).text())
        }
    }

    /// Runs the search after the debounce delay.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let delay = Duration::from_millis(settings(cx).search.debounce_ms);
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |view, cx| view.run(false, cx)).ok();
        }));
    }

    fn stop(&mut self) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        self.task = None;
        self.debounce = None;
    }

    /// Starts the search now (unless the same query already ran, or
    /// `force`), cancelling one in flight.
    pub fn run(&mut self, force: bool, cx: &mut Context<Self>) {
        self.debounce = None;
        let query = self.build_query(cx);
        if !force && self.last_query.as_ref() == Some(&query) {
            return;
        }
        self.stop();
        self.last_query = Some(query.clone());
        self.results.clear();
        self.selected = None;
        self.rebuild_rows();
        let Some(root) = self.root.clone() else {
            self.status = Status::Idle;
            cx.notify();
            return;
        };
        if query.pattern.is_empty() {
            self.status = Status::Idle;
            cx.notify();
            return;
        }
        if let Err(err) = query.validate() {
            self.status = Status::Invalid(short_error(&err.to_string()).into());
            cx.notify();
            return;
        }
        self.status = Status::Searching;
        let token = CancelToken::new();
        self.cancel = Some(token.clone());
        let (tx, rx) = async_channel::unbounded::<FileMatches>();
        let buffer_query = query.clone();
        let search = cx.background_spawn(async move {
            arcode_search::search(&root, &query, &token, |file| match tx.send_blocking(file) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            })
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                cx.background_executor().timer(BATCH_DELAY).await;
                let mut batch = vec![first];
                while let Ok(file) = rx.try_recv() {
                    batch.push(file);
                }
                let alive = this.update(cx, |view, cx| {
                    for file in batch {
                        if let Some(file) = view.prefer_buffer(file, &buffer_query, cx) {
                            view.results.insert(file);
                        }
                    }
                    view.rebuild_rows();
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let outcome = search.await;
            if outcome.is_ok()
                && buffers::add_unsaved(&this, buffer_query, cx)
                    .await
                    .is_none()
            {
                return;
            }
            this.update(cx, |view, cx| {
                view.status = match outcome {
                    Ok(stats) => Status::Done(stats),
                    Err(err) => Status::Invalid(short_error(&err.to_string()).into()),
                };
                view.cancel = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Raises the result limit and searches again.
    pub fn show_more(&mut self, cx: &mut Context<Self>) {
        self.limit = self
            .limit
            .saturating_add(settings(cx).search.max_results)
            .min(arcode_config::SearchSettings::MAX_RESULTS * 10);
        self.run(true, cx);
    }

    /// Clears the query and the results.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| f.set_text("", cx));
        self.notice = None;
        self.run(true, cx);
    }

    fn rebuild_rows(&mut self) {
        self.rows = self.results.rows();
        if self.selected.is_some_and(|s| s >= self.rows.len()) {
            self.selected = None;
        }
    }

    /// Collapses or expands every file.
    pub fn toggle_collapse_all(&mut self, cx: &mut Context<Self>) {
        self.results.toggle_all();
        self.rebuild_rows();
        cx.notify();
    }
}
