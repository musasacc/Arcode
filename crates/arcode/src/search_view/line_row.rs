//! A result line of the Search view: the matched text highlighted, or a
//! preview of the replacement (match struck through, new text after it).

use std::ops::Range;

use arcode_search::{LineReplacer, SearchQuery};
use arcode_ui_kit::metrics::ROW_RADIUS;
use arcode_ui_kit::{readout, theme};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, SharedString, StrikethroughStyle, StyledText,
    div, prelude::*, px,
};

use super::ProjectSearch;
use super::render::ROW_HEIGHT;
use super::results::{Row, char_to_byte_ranges, trimmed_line};

/// What a span of a preview line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    /// A match (struck through when a replacement follows).
    Match,
    /// Replacement text inserted after its match.
    Insert,
}

/// The display text of a result line and its styled spans (byte ranges).
/// With `replacements` (one per match) each match is followed by its
/// replacement, as a preview.
pub fn preview_line(
    text: &str,
    ranges: &[Range<usize>],
    replacements: Option<&[String]>,
) -> (String, Vec<(Range<usize>, Span)>) {
    let bytes = char_to_byte_ranges(text, ranges);
    let Some(replacements) = replacements else {
        return (
            text.to_owned(),
            bytes.into_iter().map(|r| (r, Span::Match)).collect(),
        );
    };
    let mut out = String::with_capacity(text.len() + 16);
    let mut spans = Vec::new();
    let mut pos = 0;
    for (ix, r) in bytes.into_iter().enumerate() {
        out.push_str(&text[pos..r.start]);
        let start = out.len();
        out.push_str(&text[r.clone()]);
        spans.push((start..out.len(), Span::Match));
        let new = replacements.get(ix).map(String::as_str).unwrap_or("");
        if !new.is_empty() {
            let start = out.len();
            out.push_str(new);
            spans.push((start..out.len(), Span::Insert));
        }
        pos = r.end;
    }
    out.push_str(&text[pos..]);
    (out, spans)
}

/// The compiled replace preview (see [`ProjectSearch::refresh_preview`]).
pub struct Preview {
    query: SearchQuery,
    replacement: String,
    replacer: Option<LineReplacer>,
}

impl ProjectSearch {
    /// Compiles the replace preview's query once per change of the query
    /// or replacement, not once per row and frame.
    pub(super) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let replacement = self.replacement.read(cx).text().to_string();
        if !self.show_replace || replacement.is_empty() {
            self.preview = None;
            return;
        }
        let query = self.build_query(cx);
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.query == query && p.replacement == replacement)
        {
            return;
        }
        let replacer = LineReplacer::new(&query, &replacement).ok();
        self.preview = Some(Preview {
            query,
            replacement,
            replacer,
        });
    }

    pub(super) fn render_line_row(
        &self,
        ix: usize,
        row: Row,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let Some((_, line)) = self.results.line(row) else {
            return div().into_any_element();
        };
        let (text, ranges) = trimmed_line(line);
        let expanded: Option<Vec<String>> = self.preview.as_ref().map(|p| {
            let found = p
                .replacer
                .as_ref()
                .and_then(|r| r.replacements(&text).ok())
                .unwrap_or_default();
            ranges
                .iter()
                .map(|r| {
                    found
                        .iter()
                        .find(|(fr, _)| fr == r)
                        .map(|(_, s)| s.clone())
                        .unwrap_or_else(|| p.replacement.clone())
                })
                .collect()
        });
        let (shown, spans) = preview_line(&text, &ranges, expanded.as_deref());
        let struck = expanded.is_some();
        let styles = spans.into_iter().map(|(r, span)| {
            let style = match span {
                Span::Match if struck => HighlightStyle {
                    background_color: Some(c.diff_deleted.opacity(0.25)),
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(c.text_muted),
                    }),
                    ..Default::default()
                },
                Span::Match => HighlightStyle {
                    background_color: Some(c.warning.opacity(0.3)),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
                Span::Insert => HighlightStyle {
                    background_color: Some(c.diff_added.opacity(0.3)),
                    ..Default::default()
                },
            };
            (r, style)
        });
        let styled = StyledText::new(SharedString::from(shown)).with_highlights(styles);
        let selected = self.selected == Some(ix);
        let row = div()
            .id(ix)
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW_HEIGHT))
            .pl(px(30.))
            .pr_2()
            .w_full()
            .rounded(ROW_RADIUS)
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .text_color(c.text)
            .when(selected, |el| el.bg(c.pill))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .child(div().flex_1().min_w_0().overflow_hidden().child(styled))
            .child(readout((line.line + 1).to_string()))
            .on_click(cx.listener(move |view, _, _, cx| view.open_row(ix, cx)));
        crate::sidebar::inset(row).into_any_element()
    }
}
