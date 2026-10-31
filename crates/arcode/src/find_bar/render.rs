//! Drawing the find bar: a query row and an optional replace row.

use arcode_ui_kit::{
    IconName, icon_button, mono_font_family, popover, theme, type_scale, ui_font_family,
};
use gpui::{Context, Window, div, prelude::*, px};

use super::{
    CONTEXT, CloseFindBar, FindBar, REPLACE_CONTEXT, ReplaceAllInFile, ReplaceNext, ToggleCase,
    ToggleRegex, ToggleWord, status_text,
};
use crate::actions::{FindNext, FindPrevious, shortcut_label};
use crate::search_widgets::{SearchOption, field_box, option_toggles};

/// Width of the input boxes.
const FIELD_WIDTH: f32 = 300.;
/// The narrowest an input box gets in a narrow pane; the status shrinks
/// first.
const FIELD_MIN_WIDTH: f32 = 150.;
/// Room for the status ("12 of 345") even when it is shorter, so the bar
/// keeps its width while the count changes.
const STATUS_MIN_WIDTH: f32 = 84.;
/// How much faster than the query field the status gives way in a narrow
/// pane.
const STATUS_SHRINK: f32 = 8.;

impl FindBar {
    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let (text, color) = if let Some(error) = &self.error {
            (error.to_string(), c.error)
        } else if let Some((notice, _)) = &self.notice {
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
        // The text's own width (the mono font advances 0.6 em per
        // character), at least the minimum: a fixed size, so the bar does
        // not change width with the text, and one that can shrink with an
        // ellipsis when the pane is narrow.
        let size = type_scale(cx).readout;
        let natural = text.chars().count() as f32 * f32::from(size) * 0.6;
        let mut status = div()
            .w(px(natural.ceil().clamp(STATUS_MIN_WIDTH, 260.)))
            .min_w_0()
            .truncate()
            .font_family(mono_font_family())
            .text_size(size)
            .text_color(color)
            .child(text);
        status.style().flex_shrink = Some(STATUS_SHRINK);
        status
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
        .bg(c.canvas)
        .w(px(FIELD_WIDTH))
        .min_w(px(FIELD_MIN_WIDTH));
        let read_only = self
            .editor
            .as_ref()
            .is_some_and(|e| e.read(cx).is_read_only());
        let find_row = div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(30.))
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
            .child(div().flex_1().min_w(px(8.)))
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
                .h(px(30.))
                .pl(px(26.))
                .child(
                    div()
                        .key_context(REPLACE_CONTEXT)
                        .on_action(cx.listener(|bar, _: &ReplaceNext, _, cx| bar.replace_next(cx)))
                        .on_action(cx.listener(|bar, _: &ReplaceAllInFile, _, cx| {
                            bar.replace_all(cx);
                        }))
                        .child(field_box(&self.replacement, false, None, window, cx).bg(c.canvas))
                        .w(px(FIELD_WIDTH))
                        .min_w(px(FIELD_MIN_WIDTH)),
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
                    el.child(
                        div()
                            .text_size(type_scale(cx).meta)
                            .text_color(c.text_muted)
                            .child("Read-only"),
                    )
                })
        });
        // A popover (docs/DESIGN.md 6.12) at the top right of the editor. It
        // stays in the pane's flow, so it never covers the first lines; the
        // editor drawn below it would cut its shadow, so it has none.
        let bar = popover(cx)
            .shadow(Vec::new())
            .flex_none()
            .max_w_full()
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(c.text)
            .child(find_row)
            .children(replace_row);
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
            .flex_none()
            .justify_end()
            .px(px(10.))
            .pt(px(6.))
            .pb(px(2.))
            .child(bar)
    }
}
