//! The right column of the editor: a code sample drawn with the live
//! theme (gutter, cursor line, selection, change bars, syntax colours), the
//! terminal colours, and the contrast of the pairs that matter for reading.

use arcode_config::theme::ContrastHint;
use arcode_ui_kit::metrics::{WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{IconName, icon, mono_font_family, theme};
use gpui::{
    App, Div, HighlightStyle, Hsla, SharedString, StyledText, UnderlineStyle, div, prelude::*, px,
};

/// The sample, as pieces of text and their capture (`None`: plain text).
const SAMPLE: &[&[(&str, Option<&str>)]] = &[
    &[("/// A warm light in the workshop.", Some("comment.doc"))],
    &[("#[derive(Debug, Clone)]", Some("attribute"))],
    &[
        ("pub struct ", Some("keyword")),
        ("Ember", Some("type")),
        ("<", Some("punctuation.bracket")),
        ("'a", Some("label")),
        ("> {", Some("punctuation.bracket")),
    ],
    &[
        ("    ", None),
        ("name", Some("property")),
        (": &", Some("operator")),
        ("'a ", Some("label")),
        ("str", Some("type.builtin")),
        (",", Some("punctuation.delimiter")),
    ],
    &[
        ("    ", None),
        ("heat", Some("property")),
        (": ", Some("punctuation.delimiter")),
        ("u32", Some("type.builtin")),
        (",", Some("punctuation.delimiter")),
    ],
    &[("}", Some("punctuation.bracket"))],
    &[],
    &[
        ("impl ", Some("keyword")),
        ("Ember", Some("type")),
        ("<'_> {", Some("punctuation.bracket")),
    ],
    &[
        ("    pub fn ", Some("keyword")),
        ("glow", Some("function")),
        ("(&", Some("punctuation.bracket")),
        ("self", Some("variable.builtin")),
        (", ", Some("punctuation.delimiter")),
        ("by", Some("variable.parameter")),
        (": ", Some("punctuation.delimiter")),
        ("u32", Some("type.builtin")),
        (") -> ", Some("operator")),
        ("String", Some("type")),
        (" {", Some("punctuation.bracket")),
    ],
    &[
        ("        let ", Some("keyword")),
        ("level", Some("variable")),
        (" = ", Some("operator")),
        ("self", Some("variable.builtin")),
        (".", Some("punctuation.delimiter")),
        ("heat", Some("property")),
        (" + ", Some("operator")),
        ("by", Some("variable.parameter")),
        (" * ", Some("operator")),
        ("2", Some("number")),
        (";", Some("punctuation.delimiter")),
    ],
    &[
        ("        ", None),
        ("format!", Some("function.macro")),
        ("(", Some("punctuation.bracket")),
        ("\"{}: {level}", Some("string")),
        ("\\n", Some("string.escape")),
        ("\"", Some("string")),
        (", ", Some("punctuation.delimiter")),
        ("self", Some("variable.builtin")),
        (".", Some("punctuation.delimiter")),
        ("name", Some("property")),
        (")", Some("punctuation.bracket")),
    ],
    &[("    }", Some("punctuation.bracket"))],
    &[("}", Some("punctuation.bracket"))],
];

/// The sample line with the cursor (zero-based).
const CURSOR_LINE: usize = 9;
/// Lines with an added bar, and the modified one.
const ADDED: [usize; 2] = [3, 4];
const MODIFIED: usize = 9;
/// The selected text: line and byte range.
const SELECTION: (usize, std::ops::Range<usize>) = (8, 11..15);
/// The word with an error underline.
const ERROR: (usize, std::ops::Range<usize>) = (10, 8..15);

fn sample_line(ix: usize, cx: &App) -> StyledText {
    let t = theme(cx);
    let c = &t.colors;
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for (piece, capture) in SAMPLE[ix] {
        let start = text.len();
        text.push_str(piece);
        let style = match capture {
            Some(capture) => t.syntax(capture),
            None => HighlightStyle {
                color: Some(c.text),
                ..Default::default()
            },
        };
        highlights.push((start..text.len(), style));
    }
    let extra = |range: std::ops::Range<usize>, style: HighlightStyle| (range, style);
    let mut overlays = Vec::new();
    if SELECTION.0 == ix {
        overlays.push(extra(
            SELECTION.1.clone(),
            HighlightStyle {
                background_color: Some(c.selection),
                ..Default::default()
            },
        ));
    }
    if ERROR.0 == ix {
        overlays.push(extra(
            ERROR.1.clone(),
            HighlightStyle {
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(c.error),
                    wavy: true,
                }),
                ..Default::default()
            },
        ));
    }
    let highlights = gpui::combine_highlights(highlights, overlays).collect::<Vec<_>>();
    StyledText::new(SharedString::from(text)).with_highlights(highlights)
}

/// The code sample in an editor panel as the focused pane draws it: the
/// neutral panel edge, and the active tab filled with `pill` (no mark).
pub(super) fn code_sample(cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let tab = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(10.))
        .rounded(px(7.))
        .bg(c.pill)
        .text_size(scale.meta)
        .font_weight(WEIGHT_MEDIUM)
        .child("ember.rs");
    let strip = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .p(px(6.))
        .border_b_1()
        .border_color(c.border)
        .child(tab)
        .child(
            div()
                .px(px(10.))
                .text_size(scale.meta)
                .text_color(c.text_muted)
                .child("main.rs"),
        );
    let mut body = div()
        .flex()
        .flex_col()
        .py(px(6.))
        .font_family(mono_font_family())
        .text_size(px(12.5));
    for ix in 0..SAMPLE.len() {
        let current = ix == CURSOR_LINE;
        let bar = if ADDED.contains(&ix) {
            Some(c.diff_added)
        } else if ix == MODIFIED {
            Some(c.diff_modified)
        } else {
            None
        };
        let number = div()
            .w(px(28.))
            .flex()
            .justify_end()
            .text_color(if current {
                c.gutter_text_active
            } else {
                c.gutter_text
            })
            .child(SharedString::from((ix + 1).to_string()));
        let lane = div()
            .w(px(3.))
            .h(px(16.))
            .mx(px(6.))
            .rounded(px(2.))
            .when_some(bar, |el, color| el.bg(color));
        let caret = current.then(|| div().w(px(2.)).h(px(16.)).bg(c.cursor));
        body = body.child(
            div()
                .flex()
                .items_center()
                .h(px(20.))
                .mx(px(1.))
                .when(current, |el| el.bg(c.line_highlight))
                .child(number)
                .child(lane)
                .child(div().whitespace_nowrap().child(sample_line(ix, cx)))
                .children(caret),
        );
    }
    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(c.border)
        .bg(c.panel())
        .text_color(c.text)
        .child(strip)
        .child(body)
}

/// The terminal colours: a prompt line and the 16 ANSI colours.
pub(super) fn terminal_sample(cx: &App) -> Div {
    let t = &theme(cx).terminal;
    let swatch = |color: Hsla| div().size(px(16.)).rounded(px(4.)).bg(color);
    let row = |range: std::ops::Range<usize>| {
        div()
            .flex()
            .gap(px(5.))
            .children(range.map(|ix| swatch(t.ansi[ix])))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme(cx).colors.border)
        .bg(t.background)
        .font_family(mono_font_family())
        .text_size(px(12.5))
        .child(
            div()
                .flex()
                .gap(px(6.))
                .child(div().text_color(t.ansi[2]).child("~/ember"))
                .child(div().text_color(t.foreground).child("$ cargo run"))
                .child(div().w(px(7.)).h(px(15.)).bg(t.cursor)),
        )
        .child(row(0..8))
        .child(row(8..16))
}

/// The contrast list.
pub(super) fn contrast_list(hints: &[ContrastHint], cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let mut list = div().flex().flex_col().child(
        div()
            .pt(px(16.))
            .pb(px(4.))
            .text_size(scale.title)
            .font_weight(WEIGHT_SEMIBOLD)
            .child("Contrast"),
    );
    for hint in hints {
        let (glyph, color, note) = if hint.passes() {
            (IconName::CircleCheck, c.success, String::new())
        } else {
            (
                IconName::TriangleAlert,
                c.warning,
                format!("needs {}:1", hint.min),
            )
        };
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(26.))
                .child(icon(glyph).size(px(14.)).color(color))
                .child(div().flex_1().min_w_0().truncate().child(hint.label))
                .when(!note.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(SharedString::from(note)),
                    )
                })
                .child(
                    div()
                        .w(px(52.))
                        .flex()
                        .justify_end()
                        .font_family(mono_font_family())
                        .text_size(scale.meta)
                        .child(SharedString::from(format!("{:.1}:1", hint.ratio))),
                ),
        );
    }
    list.child(
        div()
            .pt(px(4.))
            .text_size(scale.meta)
            .text_color(c.text_muted)
            .child("Text should reach 4.5:1; outlines, the accent and the caret 3:1."),
    )
}
