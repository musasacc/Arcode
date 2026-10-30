//! The welcome screen shown when no project is open.

use std::path::PathBuf;

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, kbd, theme, truncated};
use gpui::{Context, FontWeight, IntoElement, MouseButton, SharedString, div, prelude::*, px};

use crate::actions::{NewProject, OpenFolder, shortcut_label};
use crate::app_state;
use crate::workspace::Workspace;

/// Most recent projects listed.
const MAX_RECENT: usize = 8;

/// Renders the welcome screen.
pub fn render(_ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let recent: Vec<(PathBuf, String)> = app_state::recent_projects(cx)
        .iter()
        .take(MAX_RECENT)
        .map(|p| (p.path.clone(), p.name.clone()))
        .collect();

    let heading = div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(44.))
                .rounded_lg()
                .bg(c.accent)
                .child(icon(IconName::Type).size(px(24.)).color(c.accent_text)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(28.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c.text)
                        .child("Arcode"),
                )
                .child(
                    div()
                        .text_color(c.text_muted)
                        .child("A fast, native code editor"),
                ),
        );

    let actions = div()
        .flex()
        .gap_2()
        .child(
            div().rounded_md().border_1().border_color(c.accent).child(
                button("welcome-open", "Open Folder…")
                    .icon(IconName::FolderOpen)
                    .style(ButtonStyle::Primary)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
            ),
        )
        .child(
            div().rounded_md().border_1().border_color(c.border).child(
                button("welcome-new", "New Project…")
                    .icon(IconName::Plus)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewProject), cx)),
            ),
        );

    let section_title = |title: &'static str| {
        div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c.text_muted)
            .child(title.to_uppercase())
    };

    let recent_list = if recent.is_empty() {
        div()
            .text_sm()
            .text_color(c.text_disabled)
            .child("Projects you open will show up here.")
    } else {
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .children(recent.into_iter().enumerate().map(|(ix, (path, name))| {
                let group = SharedString::from(format!("recent-{ix}"));
                let open_path = path.clone();
                div()
                    .id(("welcome-recent", ix))
                    .group(group.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(30.))
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .child(icon(IconName::Folder).size(px(14.)).color(c.accent))
                    .child(div().flex_none().text_color(c.text).child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_xs()
                            .text_color(c.text_muted)
                            .child(truncated(path.display().to_string()).middle()),
                    )
                    .child(
                        div()
                            .id(("welcome-remove", ix))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(20.))
                            .rounded_sm()
                            .opacity(0.)
                            .group_hover(group, |s| s.opacity(1.))
                            .hover(|s| s.bg(c.surface_raised))
                            .child(icon(IconName::X).size(px(12.)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                app_state::remove_recent(&path, cx);
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.open_project(open_path.clone(), window, cx)
                    }))
            }))
    };

    let hint = |label: &'static str, keys: &str| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .text_sm()
            .text_color(c.text_muted)
            .child(label)
            .child(kbd(shortcut_label(keys)))
    };
    let hints = div()
        .w(px(300.))
        .flex()
        .flex_col()
        .gap_2()
        .child(hint("Open folder", "secondary-o"))
        .child(hint("Find file", "secondary-p"))
        .child(hint("Toggle sidebar", "secondary-b"))
        .child(hint("Toggle panel", "secondary-j"))
        .child(hint("Split editor", "secondary-\\"))
        .child(hint("Settings", "secondary-,"));

    div()
        .id("welcome")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .justify_center()
        .child(
            div()
                .w(px(560.))
                .max_w_full()
                .px_6()
                .pt(px(96.))
                .pb_8()
                .flex()
                .flex_col()
                .gap_8()
                .child(heading)
                .child(actions)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_title("Recent projects"))
                        .child(recent_list),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_title("Shortcuts"))
                        .child(hints),
                ),
        )
}
