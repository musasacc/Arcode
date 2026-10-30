//! The file finder (Ctrl+P): a modal with a query field and the project's
//! files ranked fuzzily. Ranking runs on a background thread.

use std::path::PathBuf;
use std::sync::Arc;

use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, HighlightStyle,
    KeyBinding, ScrollStrategy, SharedString, StyledText, Subscription, Task,
    UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::file_icons::file_icon;
use crate::fuzzy::{FileMatch, ranges_within, rank};
use crate::text_field::{TextChanged, TextField};

actions!(finder, [SelectPrev, SelectNext, Confirm, Dismiss]);

const CONTEXT: &str = "Finder";
const MAX_RESULTS: usize = 200;
const ROW_HEIGHT: f32 = 28.;
const MAX_VISIBLE_ROWS: usize = 12;

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

    fn update_matches(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        let files = self.files.clone();
        let work = cx.background_spawn(async move { rank(&files, &query, MAX_RESULTS) });
        self.match_task = Some(cx.spawn(async move |this, cx| {
            let matches = work.await;
            this.update(cx, |finder, cx| {
                finder.matches = matches;
                finder.selected = 0;
                finder.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            })
            .ok();
        }));
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            self.selected = ix.min(self.matches.len() - 1);
            self.scroll
                .scroll_to_item(self.selected, ScrollStrategy::Top);
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
        let style = HighlightStyle {
            color: Some(c.accent),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let highlight = |offset: usize, len: usize| {
            ranges_within(&m.highlights, offset, len)
                .into_iter()
                .map(|r| (r, style))
                .collect::<Vec<_>>()
        };
        let name_text = StyledText::new(SharedString::from(name.to_string()))
            .with_highlights(highlight(split, name.len()));
        let dir_trimmed = dir.trim_end_matches('/');
        let dir_text = StyledText::new(SharedString::from(dir_trimmed.to_string()))
            .with_highlights(highlight(0, dir_trimmed.len()));
        let (icon_name, icon_color) = file_icon(name, &c);
        let selected = ix == self.selected;
        div()
            .id(ix)
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_color(c.text)
            .when(selected, |el| el.bg(c.selection))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .child(icon(icon_name).size(px(14.)).color(icon_color))
            .child(div().flex_none().whitespace_nowrap().child(name_text))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(dir_text),
            )
            .on_click(cx.listener(move |finder, _, _, cx| finder.confirm_index(ix, cx)))
            .into_any_element()
    }
}

impl Render for Finder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let count = self.matches.len();
        let list_height = px(ROW_HEIGHT * count.clamp(1, MAX_VISIBLE_ROWS) as f32 + 8.);
        let status: Option<SharedString> = if self.loading && self.files.is_empty() {
            Some("Indexing project files…".into())
        } else if count == 0 {
            Some("No matching files".into())
        } else {
            None
        };
        let input_row = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(40.))
            .px_3()
            .border_b_1()
            .border_color(c.border)
            .child(icon(IconName::Search).size(px(16.)).color(c.text_muted))
            .child(self.input.clone())
            .child(kbd("Esc"));
        let body = match status {
            Some(text) => div()
                .h(list_height)
                .flex()
                .items_center()
                .px_4()
                .text_sm()
                .text_color(c.text_muted)
                .child(text)
                .into_any_element(),
            None => uniform_list(
                "finder-results",
                count,
                cx.processor(|finder, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|ix| finder.render_row(ix, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .p_1()
            .h(list_height)
            .text_sm()
            .into_any_element(),
        };
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .w(px(600.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .shadow_md()
            .child(input_row)
            .child(body)
    }
}
