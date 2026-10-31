//! Rendering of the [`NavPicker`]: the filter field and the rows (file
//! headers, symbols with their kind, references with a preview line).

use arcode_ui_kit::metrics::WEIGHT_SEMIBOLD;
use arcode_ui_kit::{IconName, badge, icon, readout, theme, truncated, type_scale};
use gpui::{
    AnyElement, Context, HighlightStyle, IntoElement, Render, SharedString, Window, div,
    prelude::*, px, relative, uniform_list,
};

use super::picker::{NavPicker, Row};
use crate::file_icons::file_icon;
use crate::picker_frame::{
    ROW_H, count_status, empty_state, footer, input_row, match_style, picker_frame, picker_list,
    picker_row, row_detail, row_icon,
};

/// Key context of the picker (shares the finder's key bindings).
const CONTEXT: &str = "Finder";
const MAX_VISIBLE_ROWS: usize = 12;

impl NavPicker {
    fn render_header(
        &self,
        row_ix: usize,
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
            .id(("nav-header", row_ix))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(ROW_H))
            .w_full()
            .px(px(10.))
            .overflow_hidden()
            .child(icon(icon_name).size(px(15.)).color(icon_color))
            .child(
                div()
                    .flex_none()
                    .text_size(type_scale(cx).meta)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .child(SharedString::from(name)),
            )
            .child(row_detail(truncated(dir).start(), cx))
            .child(badge(count.to_string()))
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
        let strong = match_style(cx);
        let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> =
            matches.iter().map(|r| (r.clone(), strong)).collect();
        if let Some(r) = &item.highlight {
            // The reference itself in its preview line: a solid band.
            let style = HighlightStyle {
                background_color: Some(c.selection),
                font_weight: Some(WEIGHT_SEMIBOLD),
                ..Default::default()
            };
            highlights.push((r.clone(), style));
        }
        highlights.sort_by_key(|(r, _)| r.start);
        highlights.dedup_by(|b, a| b.0.start < a.0.end);
        let label = truncated(item.label.clone()).highlights(highlights);
        let grouped = self.options.grouped;
        // The kind's Lucide icon in its syntax colour (`accent` when
        // selected); symbols without a kind keep the column.
        let kind = match item.kind {
            Some(k) => {
                let color = t.syntax(k.capture).color.unwrap_or(c.text_muted);
                Some(row_icon(k.icon(), selected, Some(color), cx).into_any_element())
            }
            None if !grouped => Some(div().flex_none().w(px(15.)).into_any_element()),
            None => None,
        };
        let line = (item.target.line() + 1).to_string();
        picker_row(row_ix, selected, cx)
            .pl(px(10.
                + 14. * item.depth.min(8) as f32
                + if grouped { 22. } else { 0. }))
            .children(kind)
            .child(
                div()
                    .when(grouped, |d| d.flex_1().min_w_0().font_family(mono(cx)))
                    .when(!grouped, |d| d.min_w_0().max_w(relative(0.7)))
                    .overflow_hidden()
                    .child(label),
            )
            .when(!grouped, |el| {
                el.child(row_detail(
                    truncated(item.detail.clone().unwrap_or_default()),
                    cx,
                ))
            })
            .when(grouped, |el| el.child(readout(line)))
            .on_click(cx.listener(move |picker, _, _, cx| picker.confirm_row(row_ix, cx)))
            .into_any_element()
    }
}

/// The editor font, for code in reference previews.
fn mono(cx: &gpui::App) -> SharedString {
    SharedString::from(arcode_ui_kit::settings(cx).editor.font_family.clone())
}

impl Render for NavPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows.len();
        let status: Option<SharedString> = if self.loading && self.items.is_empty() {
            Some("Loading…".into())
        } else if count == 0 {
            Some("No results".into())
        } else {
            None
        };
        let body = match status {
            Some(text) => empty_state(text, cx).into_any_element(),
            None => {
                let list = uniform_list(
                    "nav-results",
                    count,
                    cx.processor(|picker, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|ix| match picker.rows[ix].clone() {
                                Row::Header { path, count } => {
                                    picker.render_header(ix, &path, count, cx)
                                }
                                Row::Item { ix: item, matches } => {
                                    picker.render_item(ix, item, &matches, cx)
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone());
                picker_list(list, count, MAX_VISIBLE_ROWS).into_any_element()
            }
        };
        let shown = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Item { .. }))
            .count();
        let footer_status = (!self.loading).then(|| count_status(None, shown, self.items.len()));
        let mode = if self.options.grouped {
            "Locations"
        } else {
            "Symbols"
        };
        picker_frame(px(680.), cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .child(input_row(
                IconName::Search,
                None,
                self.input.clone(),
                Some(mode.into()),
                cx,
            ))
            .child(body)
            .child(footer("go", footer_status, cx))
    }
}
