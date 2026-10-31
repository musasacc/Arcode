//! The frame and rows shared by the command palette and every picker (file
//! finder, branch, language, indentation and theme pickers, Go to symbol):
//! docs/DESIGN.md 6.11. A raised overlay with an input row, inset 32 px row
//! chips and a footer with the keys and a count. Modal dialogs (6.12) use
//! the same frame with a title row and a button row.

use arcode_ui_kit::metrics::{TITLE_BAR_H, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD};
use arcode_ui_kit::{
    Icon, IconName, Truncated, icon, kbd, list_row, overlay, readout, theme, type_scale,
    ui_font_family,
};
use gpui::{
    AnyElement, App, Div, ElementId, HighlightStyle, IntoElement, Pixels, ScrollStrategy,
    SharedString, Stateful, UniformList, div, prelude::*, px, relative,
};

/// Width of the palette and the file finder.
pub const PALETTE_W: Pixels = px(600.);
/// Height of the input row.
pub const INPUT_ROW_H: Pixels = px(46.);
/// Height of the footer.
pub const FOOTER_H: Pixels = px(30.);
/// Height of a row (32).
pub const ROW_H: f32 = 32.;
/// Space above the first row and below the last one; rows are also inset
/// this much from the sides, so nothing touches the rounded corners.
pub const LIST_PAD: f32 = 6.;
/// Gap between the overlay and the title bar.
pub const TOP_GAP: f32 = 4.;

/// The full-window layer that holds a picker or a dialog: no dim (DESIGN
/// 5.2), it only blocks the workspace below and centres its child 4 px
/// under the title bar. Callers add a mouse-down handler that closes the
/// overlay and the panel as its child.
pub fn picker_layer(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .justify_center()
        .pt(TITLE_BAR_H + px(TOP_GAP))
}

/// The overlay frame: `panel_raised`, 1 px `border_strong`, radius 12, the
/// large shadow (none in low-end mode), `width` wide, body text in the UI
/// font. Children are clipped; rows stay inset from the corners.
pub fn picker_frame(width: Pixels, cx: &App) -> Div {
    overlay(cx)
        .w(width)
        .font_family(ui_font_family(cx))
        .text_size(type_scale(cx).body)
        .text_color(theme(cx).colors.text)
}

/// The input row: a 16 px `accent` glyph, an optional muted `prefix` (`>`
/// in the palette), the text field in the input size, and an optional mode
/// badge at the right ("Commands"). A hairline separates it from the list.
pub fn input_row(
    glyph: IconName,
    prefix: Option<&'static str>,
    input: impl IntoElement,
    mode: Option<SharedString>,
    cx: &App,
) -> Div {
    let c = &theme(cx).colors;
    let scale = type_scale(cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(INPUT_ROW_H)
        .px(px(14.))
        .border_b_1()
        .border_color(c.border)
        .child(icon(glyph).size(px(16.)).color(c.accent))
        .when_some(prefix, |el, prefix| {
            el.child(
                div()
                    .flex_none()
                    .mr(px(-4.))
                    .text_size(scale.input)
                    .text_color(c.text_muted)
                    .child(prefix),
            )
        })
        .child(
            // The field fills the row's height: it centres its own line.
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .text_size(scale.input)
                .child(input),
        )
        .when_some(mode, |el, mode| el.child(mode_badge(mode, cx)))
}

/// The mode badge at the right of the input row ("Commands", "Files").
pub fn mode_badge(text: impl Into<SharedString>, cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.))
        .px(px(7.))
        .rounded(px(5.))
        .bg(c.pill)
        .text_size(type_scale(cx).meta)
        .font_weight(WEIGHT_MEDIUM)
        .text_color(c.text_muted)
        .child(text.into())
}

/// A result row: 32 tall, radius 8, gap 10, solid hover, `pill` fill and
/// Medium text when `selected`. It fills the list's width; the list's
/// [`LIST_PAD`] padding insets it 6 px from the frame.
pub fn picker_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    list_row(id, selected, cx)
        .h(px(ROW_H))
        .mx(px(0.))
        .w_full()
        .px(px(10.))
        .gap(px(10.))
        .rounded(px(8.))
        .overflow_hidden()
}

/// The 15 px row icon: `accent` on the selected row, else `color` (muted
/// when `None`).
pub fn row_icon(name: IconName, selected: bool, color: Option<gpui::Hsla>, cx: &App) -> Icon {
    let c = &theme(cx).colors;
    let color = if selected {
        c.accent
    } else {
        color.unwrap_or(c.text_muted)
    };
    icon(name).size(px(15.)).color(color)
}

/// Matched characters: `accent` SemiBold.
pub fn match_style(cx: &App) -> HighlightStyle {
    HighlightStyle {
        color: Some(theme(cx).colors.accent),
        font_weight: Some(WEIGHT_SEMIBOLD),
        ..Default::default()
    }
}

/// A row's main text (a file or branch name): whole while it fits, else
/// cut with an ellipsis. With a detail after it, it keeps to 70 % of the
/// row so the detail stays readable. `Truncated` rather than a GPUI
/// `truncate()`: that one measures the text once, unshrunk, and then only
/// clips it.
pub fn row_label(text: Truncated, has_detail: bool) -> Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .when(has_detail, |el| el.max_w(relative(0.7)))
        .child(text)
}

/// Muted secondary text after a row's label (a folder, a branch summary),
/// in the meta size, taking the rest of the row and cut with an ellipsis.
pub fn row_detail(text: Truncated, cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_size(type_scale(cx).meta)
        .font_weight(gpui::FontWeight::NORMAL)
        .text_color(c.text_muted)
        .child(text)
}

/// The height of a list showing `rows` rows (at least one, at most `max`),
/// with its top and bottom padding.
pub fn list_height(rows: usize, max: usize) -> Pixels {
    px(ROW_H * rows.clamp(1, max.max(1)) as f32 + 2. * LIST_PAD)
}

/// The list body of the palette, the finder and Go to symbol: `list`, a
/// `uniform_list` of [`ROW_H`] rows (`rows` of them, at most `max` shown),
/// inset [`LIST_PAD`] on every side. The inset sits on a wrapper because
/// GPUI 0.2.2's `uniform_list` adds its top padding twice and counts the
/// bottom padding against the last visible row, so scrolling to that row
/// would jump the list.
pub fn picker_list(list: UniformList, rows: usize, max: usize) -> Div {
    div()
        .flex_none()
        .p(px(LIST_PAD))
        .h(list_height(rows, max))
        .child(list.size_full())
}

/// How to scroll a `uniform_list` to the row the keyboard moved to from
/// `from`: a row below comes in at the bottom and a row above at the top,
/// so the list moves by one row instead of a page. A row already in view
/// does not move the list.
pub fn scroll_strategy(from: usize, to: usize) -> ScrollStrategy {
    if to > from {
        ScrollStrategy::Bottom
    } else {
        ScrollStrategy::Top
    }
}

/// The scroll offset (negative, as GPUI stores it) that shows `row` of a
/// padded list of [`ROW_H`] rows showing at most `max` rows, scrolled to
/// `offset`, moving as little as possible. For lists in a scrolling div
/// (the branch and list pickers) whose padding scrolls with the rows.
pub fn offset_showing(offset: f32, row: usize, max: usize) -> f32 {
    let viewport = ROW_H * max as f32 + 2. * LIST_PAD;
    let top = LIST_PAD + row as f32 * ROW_H;
    let bottom = top + ROW_H;
    if top + offset < 0. {
        -(top - LIST_PAD)
    } else if bottom + offset > viewport {
        viewport - LIST_PAD - bottom
    } else {
        offset
    }
}

/// The body shown instead of rows ("No matching commands", "Loading…").
pub fn empty_state(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(ROW_H + 2. * LIST_PAD))
        .px(px(16.))
        .text_color(theme(cx).colors.text_muted)
        .child(text.into())
}

/// The footer: a hairline above, the keys (`↑` `↓` move, `Enter` `verb`,
/// `Esc` close) at the left in the meta size, `status` at the right.
pub fn footer(verb: &'static str, status: Option<AnyElement>, cx: &App) -> Div {
    let c = &theme(cx).colors;
    let hint = |keys: &'static [&'static str], word: &'static str| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.))
            .children(keys.iter().map(|k| kbd(*k)))
            .child(word)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(14.))
        .h(FOOTER_H)
        .px(px(14.))
        .border_t_1()
        .border_color(c.border)
        .text_size(type_scale(cx).meta)
        .text_color(c.text_muted)
        .child(hint(&["↑", "↓"], "move"))
        .child(hint(&["Enter"], verb))
        .child(hint(&["Esc"], "close"))
        .child(div().flex_1())
        .children(status)
}

/// A dialog's frame: the overlay frame, `width` wide.
pub fn dialog_frame(width: Pixels, cx: &App) -> Div {
    picker_frame(width, cx)
}

/// A dialog's title: the title size SemiBold, 16 px from the edges.
pub fn dialog_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(16.))
        .pt(px(14.))
        .pb(px(8.))
        .text_size(type_scale(cx).title)
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(theme(cx).colors.text)
        .child(text.into())
}

/// A dialog's last row: an optional muted `hint` at the left (meta size),
/// the buttons at the right (at most one Primary, last).
pub fn dialog_buttons(
    hint: Option<AnyElement>,
    buttons: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(16.))
        .pt(px(12.))
        .pb(px(14.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(type_scale(cx).meta)
                .text_color(theme(cx).colors.text_muted)
                .children(hint),
        )
        .children(buttons)
}

/// `"{prefix} · {shown} of {total}"` with the numbers in mono; the prefix
/// is left out when `None`.
pub fn count_status(prefix: Option<&'static str>, shown: usize, total: usize) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .whitespace_nowrap()
        .when_some(prefix, |el, prefix| el.child(format!("{prefix} ·")))
        .child(readout(group_digits(shown)))
        .child("of")
        .child(readout(group_digits(total)))
        .into_any_element()
}

/// `1204` as `1,204`.
pub fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(214), "214");
        assert_eq!(group_digits(1204), "1,204");
        assert_eq!(group_digits(1_000_000), "1,000,000");
    }

    #[test]
    fn keyboard_moves_scroll_by_one_row() {
        assert_eq!(scroll_strategy(8, 9), ScrollStrategy::Bottom);
        assert_eq!(scroll_strategy(9, 8), ScrollStrategy::Top);
        // Wrapping round from the last row to the first.
        assert_eq!(scroll_strategy(15, 0), ScrollStrategy::Top);
        assert_eq!(scroll_strategy(0, 15), ScrollStrategy::Bottom);
    }

    #[test]
    fn offset_showing_moves_as_little_as_needed() {
        // Rows 0..10 fit without scrolling.
        assert_eq!(offset_showing(0., 9, 10), 0.);
        // Row 10 becomes the last visible row, then row 12.
        assert_eq!(offset_showing(0., 10, 10), -ROW_H);
        assert_eq!(offset_showing(-ROW_H, 12, 10), -3. * ROW_H);
        // Going back up shows the row at the top.
        assert_eq!(offset_showing(-3. * ROW_H, 1, 10), -ROW_H);
        assert_eq!(offset_showing(-3. * ROW_H, 0, 10), 0.);
        // Already visible: no change.
        assert_eq!(offset_showing(-3. * ROW_H, 7, 10), -3. * ROW_H);
    }

    #[test]
    fn list_height_counts_rows_and_padding() {
        assert_eq!(list_height(0, 12), px(ROW_H + 12.));
        assert_eq!(list_height(3, 12), px(3. * ROW_H + 12.));
        assert_eq!(list_height(40, 12), px(12. * ROW_H + 12.));
    }
}
