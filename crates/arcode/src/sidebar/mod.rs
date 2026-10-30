//! The sidebar: the file tree for the Files activity, placeholders for the
//! activities that later milestones fill in.

pub mod file_tree;

use arcode_ui_kit::{ButtonStyle, button, icon, theme};
use gpui::{App, IntoElement, div, prelude::*, px};

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
                .child(div().text_sm().child(activity.label()))
                .child(div().text_xs().text_color(c.text_disabled).child("Coming soon")),
        )
}

/// Content of the Files activity when no folder is open.
pub fn no_folder(cx: &App) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header("Explorer", cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .text_sm()
                .text_color(c.text_muted)
                .child("No folder is open.")
                .child(
                    button("sidebar-open-folder", "Open Folder")
                        .style(ButtonStyle::Primary)
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
                ),
        )
}

/// A sidebar section title such as `EXPLORER`.
pub fn header(title: &str, cx: &App) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(34.))
        .px_3()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(c.text_muted)
        .child(title.to_uppercase())
}
