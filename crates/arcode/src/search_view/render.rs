//! Drawing the Search view: the input section, the status line and the
//! result list (only visible rows are laid out).

use std::ops::Range;

use arcode_ui_kit::metrics::{ROW_RADIUS, WEIGHT_MEDIUM, icon_size};
use arcode_ui_kit::{IconName, badge, icon, icon_button, theme, type_scale};
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px, uniform_list};

use super::results::{Row, split_path};
use super::{
    CONTEXT, GLOB_CONTEXT, ProjectSearch, REPLACE_CONTEXT, ReplaceAllMatches, RunSearch, Status,
};
use crate::file_icons::file_icon;
use crate::find_bar::{ToggleCase, ToggleRegex, ToggleWord};
use crate::search_widgets::{SearchOption, field_box, option_toggles};

/// Height of a result row.
pub(super) const ROW_HEIGHT: f32 = 24.;

impl ProjectSearch {
    fn render_inputs(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let toggles = option_toggles("search-opt", self.options, cx, move |option, _, cx| {
            this.update(cx, |view, cx| view.toggle_option(option, cx))
                .ok();
        });
        let invalid = matches!(self.status, Status::Invalid(_));
        let query = field_box(
            &self.query,
            invalid,
            Some(toggles.into_any_element()),
            window,
            cx,
        );
        let replace = self.show_replace.then(|| {
            let button = icon_button("search-replace-all", IconName::ReplaceAll)
                .size(px(22.))
                .tooltip("Replace All")
                .on_click(cx.listener(|view, _, window, cx| view.replace_all(window, cx)));
            field_box(
                &self.replacement,
                false,
                Some(button.into_any_element()),
                window,
                cx,
            )
            .key_context(REPLACE_CONTEXT)
        });
        let globs = self.show_globs.then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .key_context(GLOB_CONTEXT)
                .child(field_box(&self.include, false, None, window, cx))
                .child(field_box(&self.exclude, false, None, window, cx))
        });
        div()
            .flex()
            .gap_1()
            .px_2()
            .child(
                icon_button(
                    "search-toggle-replace",
                    if self.show_replace {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                )
                .size(px(20.))
                .tooltip("Toggle Replace")
                .on_click(cx.listener(|view, _, _, cx| {
                    view.show_replace = !view.show_replace;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(query)
                    .children(replace)
                    .child(
                        div().flex().justify_end().child(
                            icon_button("search-toggle-globs", IconName::Ellipsis)
                                .size(px(20.))
                                .active(self.show_globs)
                                .tooltip("Toggle Search Details")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.show_globs = !view.show_globs;
                                    cx.notify();
                                })),
                        ),
                    )
                    .children(globs),
            )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let files = self.results.files().len();
        let matches = self.results.match_count();
        let counts = || {
            format!(
                "{matches} result{} in {files} file{}",
                if matches == 1 { "" } else { "s" },
                if files == 1 { "" } else { "s" }
            )
        };
        let (text, color): (String, _) = match &self.status {
            Status::Idle if self.root.is_none() => {
                ("Open a folder to search it.".into(), c.text_muted)
            }
            Status::Idle => (String::new(), c.text_muted),
            Status::Searching if matches == 0 => ("Searching…".into(), c.text_muted),
            Status::Searching => (format!("{} – searching…", counts()), c.text_muted),
            Status::Done(_) if matches == 0 => ("No results.".into(), c.text_muted),
            Status::Done(_) => (counts(), c.text_muted),
            Status::Invalid(err) => (err.to_string(), c.error),
        };
        let truncated = matches!(&self.status, Status::Done(stats) if stats.truncated);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_1()
            .text_size(type_scale(cx).meta)
            .when(!text.is_empty(), |el| {
                el.child(div().text_color(color).child(text))
            })
            .when(self.replacing, |el| {
                el.child(div().text_color(c.text_muted).child("Replacing…"))
            })
            .when_some(self.notice.clone(), |el, notice| {
                el.child(div().text_color(c.text).child(notice))
            })
            .when(truncated, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .text_color(c.warning)
                        .child(format!("Only the first {} results are shown.", self.limit))
                        .child(
                            div()
                                .id("search-show-more")
                                .cursor_pointer()
                                .text_color(c.accent)
                                .hover(|s| s.underline())
                                .child("Show more")
                                .on_click(cx.listener(|view, _, _, cx| view.show_more(cx))),
                        ),
                )
            })
    }

    fn render_file_row(&self, ix: usize, fx: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let file = &self.results.files()[fx];
        let collapsed = self.results.is_collapsed(file);
        let (name, dir) = split_path(&file.relative);
        let (file_icon_name, file_icon_color) = file_icon(&name, &c);
        let selected = self.selected == Some(ix);
        let group = SharedString::from(format!("search-file-{fx}"));
        let row = div()
            .id(ix)
            .group(group.clone())
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(ROW_HEIGHT))
            .px_2()
            .w_full()
            .rounded(ROW_RADIUS)
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .when(selected, |el| el.bg(c.pill).font_weight(WEIGHT_MEDIUM))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .child(
                icon(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(px(14.))
                .color(c.text_muted),
            )
            .child(
                icon(file_icon_name)
                    .size(icon_size::ROW)
                    .color(file_icon_color),
            )
            .child(div().flex_none().text_color(c.text).child(name))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(type_scale(cx).meta)
                    .text_color(c.text_muted)
                    .child(dir),
            )
            .when(self.show_replace, |el| {
                el.child(
                    div().invisible().group_hover(group, |s| s.visible()).child(
                        icon_button(("search-replace-file", fx), IconName::ReplaceAll)
                            .size(px(18.))
                            .tooltip("Replace in File")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                view.replace_in_file(fx, cx)
                            })),
                    ),
                )
            })
            .child(badge(file.match_count().to_string()))
            .on_click(cx.listener(move |view, _, _, cx| view.open_row(ix, cx)));
        crate::sidebar::inset(row).into_any_element()
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.rows.get(ix) {
            Some(&Row::File(fx)) => self.render_file_row(ix, fx, cx),
            Some(&row) => self.render_line_row(ix, row, cx),
            None => div().into_any_element(),
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let has_results = !self.results.files().is_empty();
        crate::sidebar::header("Search", cx).child(
            div()
                .flex()
                .gap_0p5()
                .child(
                    icon_button("search-refresh", IconName::Refresh)
                        .size(px(24.))
                        .tooltip("Search Again")
                        .on_click(cx.listener(|view, _, _, cx| view.run(true, cx))),
                )
                .when(has_results, |el| {
                    el.child(
                        icon_button("search-collapse", IconName::ChevronsDownUp)
                            .size(px(24.))
                            .tooltip("Collapse All")
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_collapse_all(cx))),
                    )
                    .child(
                        icon_button("search-clear", IconName::X)
                            .size(px(24.))
                            .tooltip("Clear Results")
                            .on_click(cx.listener(|view, _, _, cx| view.clear(cx))),
                    )
                }),
        )
    }
}

impl Render for ProjectSearch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_preview(cx);
        let count = self.rows.len();
        let list = (count > 0).then(|| {
            uniform_list(
                "search-results",
                count,
                cx.processor(|view, range: Range<usize>, _, cx| {
                    range.map(|ix| view.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1()
            .text_size(type_scale(cx).body)
        });
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(|view, _: &RunSearch, _, cx| view.run(false, cx)))
            .on_action(
                cx.listener(|view, _: &ReplaceAllMatches, window, cx| view.replace_all(window, cx)),
            )
            .on_action(
                cx.listener(|view, _: &ToggleCase, _, cx| {
                    view.toggle_option(SearchOption::Case, cx)
                }),
            )
            .on_action(
                cx.listener(|view, _: &ToggleWord, _, cx| {
                    view.toggle_option(SearchOption::Word, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleRegex, _, cx| {
                view.toggle_option(SearchOption::Regex, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .text_size(type_scale(cx).body)
            .child(self.render_header(cx))
            .child(self.render_inputs(window, cx))
            .child(self.render_status(cx))
            .children(list)
    }
}
