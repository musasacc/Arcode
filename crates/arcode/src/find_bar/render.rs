//! Drawing the find bar: a query row and an optional replace row.

use arcode_ui_kit::{IconName, icon_button, theme};
use gpui::{Context, Window, div, prelude::*, px};

use super::{
    CONTEXT, CloseFindBar, FindBar, REPLACE_CONTEXT, ReplaceAllInFile, ReplaceNext, ToggleCase,
    ToggleRegex, ToggleWord, status_text,
};
use crate::actions::{FindNext, FindPrevious, shortcut_label};
use crate::search_widgets::{SearchOption, field_box, option_toggles};

/// Width of the input boxes.
const FIELD_WIDTH: f32 = 300.;

impl FindBar {
    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let (text, color) = if let Some(error) = &self.error {
            (error.to_string(), c.error)
        } else if let Some(notice) = &self.notice {
            (notice.to_string(), c.text_muted)
        } else if self.query_text(cx).is_empty() {
            (String::new(), c.text_muted)
        } else {
            let missing = self.matches.is_empty();
            (
                status_text(self.current, self.matches.len(), self.capped),
                if missing { c.warning } else { c.text_muted },
            )
        };
        div()
            .flex_none()
            .min_w(px(84.))
            .max_w(px(260.))
            .truncate()
            .text_xs()
            .text_color(color)
            .child(text)
    }
}

impl Render for FindBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let this = cx.entity().downgrade();
        let toggles = option_toggles("find-opt", self.options, cx, move |option, _, cx| {
            this.update(cx, |bar, cx| bar.toggle_option(option, cx))
                .ok();
        });
        let query_box = field_box(
            &self.query,
            self.error.is_some(),
            Some(toggles.into_any_element()),
            window,
            cx,
        )
        .w(px(FIELD_WIDTH));
        let read_only = self
            .editor
            .as_ref()
            .is_some_and(|e| e.read(cx).is_read_only());
        let find_row = div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(32.))
            .child(
                icon_button(
                    "find-toggle-replace",
                    if self.show_replace {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                )
                .size(px(22.))
                .tooltip("Toggle Replace")
                .on_click(cx.listener(|bar, _, _, cx| bar.toggle_replace(cx))),
            )
            .child(query_box)
            .child(self.render_status(cx))
            .child(
                icon_button("find-prev", IconName::ArrowUp)
                    .size(px(22.))
                    .tooltip("Previous Match (Shift+Enter)")
                    .on_click(cx.listener(|bar, _, _, cx| bar.go(false, cx))),
            )
            .child(
                icon_button("find-next", IconName::ArrowDown)
                    .size(px(22.))
                    .tooltip("Next Match (Enter)")
                    .on_click(cx.listener(|bar, _, _, cx| bar.go(true, cx))),
            )
            .child(div().flex_1())
            .child(
                icon_button("find-close", IconName::X)
                    .size(px(22.))
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|bar, _, _, cx| bar.close(cx))),
            );
        let replace_row = self.show_replace.then(|| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .h(px(32.))
                .pl(px(26.))
                .child(
                    div()
                        .key_context(REPLACE_CONTEXT)
                        .on_action(cx.listener(|bar, _: &ReplaceNext, _, cx| bar.replace_next(cx)))
                        .on_action(cx.listener(|bar, _: &ReplaceAllInFile, _, cx| {
                            bar.replace_all(cx);
                        }))
                        .child(field_box(&self.replacement, false, None, window, cx))
                        .w(px(FIELD_WIDTH)),
                )
                .when(!read_only, |el| {
                    el.child(
                        icon_button("find-replace-one", IconName::Replace)
                            .size(px(22.))
                            .tooltip("Replace (Enter)")
                            .on_click(cx.listener(|bar, _, _, cx| bar.replace_next(cx))),
                    )
                    .child(
                        icon_button("find-replace-all", IconName::ReplaceAll)
                            .size(px(22.))
                            .tooltip(format!(
                                "Replace All ({})",
                                shortcut_label("secondary-alt-enter")
                            ))
                            .on_click(cx.listener(|bar, _, _, cx| {
                                bar.replace_all(cx);
                            })),
                    )
                })
                .when(read_only, |el| {
                    el.child(div().text_xs().text_color(c.text_muted).child("Read-only"))
                })
        });
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|bar, _: &FindNext, _, cx| bar.go(true, cx)))
            .on_action(cx.listener(|bar, _: &FindPrevious, _, cx| bar.go(false, cx)))
            .on_action(cx.listener(|bar, _: &CloseFindBar, _, cx| bar.close(cx)))
            .on_action(cx.listener(Self::focus_next_field))
            .on_action(
                cx.listener(|bar, _: &ToggleCase, _, cx| bar.toggle_option(SearchOption::Case, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &ToggleWord, _, cx| bar.toggle_option(SearchOption::Word, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &ToggleRegex, _, cx| {
                    bar.toggle_option(SearchOption::Regex, cx)
                }),
            )
            .flex()
            .flex_col()
            .flex_none()
            .px_1()
            .py_0p5()
            .bg(c.surface)
            .border_b_1()
            .border_color(c.border)
            .text_sm()
            .child(find_row)
            .children(replace_row)
    }
}
