//! The sidebar: the file tree for the Files activity, placeholders for the
//! activities that later milestones fill in, and the title row every
//! sidebar view starts with (docs/DESIGN.md 6.4: sentence case, never
//! uppercased).

pub mod file_tree;

use arcode_ui_kit::metrics::{PANEL_TITLE_H, ROW_INSET, WEIGHT_SEMIBOLD};
use arcode_ui_kit::{
    ButtonStyle, badge, button, icon, mono_font_family, spacer, theme, truncated, type_scale,
};
use gpui::{App, Div, FontWeight, Hsla, IntoElement, div, prelude::*, px};

use crate::actions::OpenFolder;
use crate::activity_bar::Activity;

/// Default sidebar width.
pub const DEFAULT_WIDTH: f32 = 240.;
/// Narrowest the sidebar can be dragged.
pub const MIN_WIDTH: f32 = 160.;
/// Widest the sidebar can be dragged.
pub const MAX_WIDTH: f32 = 600.;

/// "Coming soon" content for activities that are not implemented yet.
pub fn placeholder(activity: Activity, cx: &App) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header(activity.label(), cx))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .px_4()
                .text_color(c.text_muted)
                .child(icon(activity.icon()).size(px(32.)).color(c.text_disabled))
                .child(div().text_size(type_scale(cx).body).child(activity.label()))
                .child(
                    div()
                        .text_size(type_scale(cx).meta)
                        .text_color(c.text_disabled)
                        .child("Coming soon"),
                ),
        )
}

/// Content of the Files activity when no folder is open.
pub fn no_folder(cx: &App) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header("Files", cx))
        .child(
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .px(px(14.))
                .py_2()
                .text_size(type_scale(cx).body)
                .text_color(c.text_muted)
                .child("No folder is open.")
                .child(
                    button("sidebar-open-folder", "Open Folder…")
                        .icon(arcode_ui_kit::IconName::FolderOpen)
                        .style(ButtonStyle::Primary)
                        .compact()
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
                ),
        )
}

/// The title row of a sidebar view ("Files", "Search", "Source control"):
/// 40 tall, title size SemiBold, sentence case. Append 24 px icon buttons
/// as children; they sit at the right. At the narrowest sidebar the title
/// ends in "…" rather than pushing the buttons out of the panel.
pub fn header(title: &str, cx: &App) -> Div {
    header_count(title, 0, cx)
}

/// Wraps a full-width list row so it sits as a chip inset 6 px from the
/// panel edges (docs/DESIGN.md 5.3: rows never touch the rounded corners).
/// Give the row `w_full()` and `rounded(ROW_RADIUS)`.
pub fn inset(row: impl IntoElement) -> Div {
    div().w_full().px(ROW_INSET).child(row)
}

/// A title row with a mono count badge after the title ("Todo 3",
/// "Packages 12"); no badge when `count` is zero. Append buttons as
/// children.
pub fn header_count(title: &str, count: usize, cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(PANEL_TITLE_H)
        .pl(px(14.))
        .pr(px(8.))
        .child(
            div()
                .flex_shrink()
                .min_w_0()
                .overflow_hidden()
                .text_size(type_scale(cx).title)
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(c.text)
                .child(truncated(title.to_owned())),
        )
        .when(count > 0, |el| {
            el.child(div().flex_none().ml(px(2.)).child(badge(count.to_string())))
        })
        .child(spacer())
}

/// A git status letter ("M", "A", "U", "D") as a readout: mono 12 bold in
/// its text-safe status color.
pub fn git_letter(letter: &'static str, color: Hsla, cx: &App) -> Div {
    div()
        .flex_none()
        .font_family(mono_font_family())
        .text_size(type_scale(cx).readout)
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter)
}

/// A row's name followed by a muted hint (a folder, a version): both end
/// in "…" when space runs out, but the hint gives way first and the name
/// only shrinks once the hint is gone, since the name is what people read.
/// Style the two parts by passing styled divs; this adds the flex sizing.
pub fn name_then_hint(name: Div, hint: Div) -> Div {
    let mut hint = hint.min_w_0().overflow_hidden();
    // Shrink is weighted by this factor, so the hint absorbs nearly all of
    // the deficit until it reaches zero width.
    hint.style().flex_shrink = Some(HINT_SHRINK);
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_1p5()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(name.flex_shrink().min_w_0().overflow_hidden())
        .child(hint)
}

/// How much faster a hint shrinks than the name before it.
const HINT_SHRINK: f32 = 10_000.;
