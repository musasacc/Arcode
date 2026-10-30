//! Rendering of the [`NavPicker`]: the filter field and the rows (file
//! headers, symbols with their kind, references with a preview line).

use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, Render, SharedString, StyledText,
    Window, div, prelude::*, px, uniform_list,
};

use super::picker::{NavPicker, Row};
use crate::file_icons::file_icon;

/// Key context of the picker (shares the finder's key bindings).
const CONTEXT: &str = "Finder";
const ROW_HEIGHT: f32 = 26.;
const MAX_VISIBLE_ROWS: usize = 14;

impl NavPicker {
    fn render_header(
        &self,
        path: &std::path::Path,
        count: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = path
            .parent()
            .map(|d| self.display_path(d))
            .unwrap_or_default();
        let (icon_name, icon_color) = file_icon(&name, &c);
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .overflow_hidden()
            .child(icon(icon_name).size(px(14.)).color(icon_color))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(name)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(SharedString::from(dir)),
            )
            .child(
                div()
                    .flex_none()
                    .px_1p5()
                    .rounded_md()
                    .bg(c.hover)
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(SharedString::from(count.to_string())),
            )
            .into_any_element()
    }

    fn render_item(
        &self,
        row_ix: usize,
        ix: usize,
        matches: &[std::ops::Range<usize>],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let c = t.colors.clone();
        let item = &self.items[ix];
        let selected = row_ix == self.selected;
        let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = matches
            .iter()
            .map(|r| {
                let style = HighlightStyle {
                    color: Some(c.accent),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                };
                (r.clone(), style)
            })
            .collect();
        if let Some(r) = &item.highlight {
            let style = HighlightStyle {
                background_color: Some(c.accent.opacity(0.25)),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            };
            highlights.push((r.clone(), style));
        }
        highlights.sort_by_key(|(r, _)| r.start);
        highlights.dedup_by(|b, a| b.0.start < a.0.end);
        let label =
            StyledText::new(SharedString::from(item.label.clone())).with_highlights(highlights);
        let kind = item.kind.map(|k| {
            let color = t.syntax(k.capture).color.unwrap_or(c.text_muted);
            div()
                .flex_none()
                .w(px(44.))
                .text_xs()
                .text_color(color)
                .child(k.label)
        });
        let grouped = self.options.grouped;
        let line = SharedString::from((item.target.line() + 1).to_string());
        div()
            .id(row_ix)
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(8.
                + 14. * item.depth.min(8) as f32
                + if grouped { 20. } else { 0. }))
            .pr_2()
            .rounded_md()
            .cursor_pointer()
            .text_color(c.text)
            .when(selected, |el| el.bg(c.selection))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .children(kind)
            .child(
                div()
                    .when(grouped, |d| d.flex_1().min_w_0().font_family(mono(cx)))
                    .when(!grouped, |d| d.flex_none().max_w(px(420.)))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .when(!grouped, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(c.text_muted)
                        .children(item.detail.clone().map(SharedString::from)),
                )
            })
            .when(grouped, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(line),
                )
            })
            .on_click(cx.listener(move |picker, _, _, cx| picker.confirm_row(row_ix, cx)))
            .into_any_element()
    }
}

fn mono(cx: &gpui::App) -> SharedString {
    SharedString::from(arcode_ui_kit::settings(cx).editor.font_family.clone())
}

impl Render for NavPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let count = self.rows.len();
        let list_height = px(ROW_HEIGHT * count.clamp(1, MAX_VISIBLE_ROWS) as f32 + 8.);
        let status: Option<SharedString> = if self.loading && self.items.is_empty() {
            Some("Loading…".into())
        } else if count == 0 {
            Some("No results".into())
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
                "nav-results",
                count,
                cx.processor(|picker, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|ix| match picker.rows[ix].clone() {
                            Row::Header { path, count } => picker.render_header(&path, count, cx),
                            Row::Item { ix: item, matches } => {
                                picker.render_item(ix, item, &matches, cx)
                            }
                        })
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
            .w(px(680.))
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
