//! The welcome screen shown when no project is open (docs/DESIGN.md
//! 6.15): the spark logo and the two ways in on the left with the recent
//! projects under them, "Get going" shortcuts on the right.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use arcode_ui_kit::metrics::{
    ROW_H_WELCOME, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, icon_size, type_scale,
};
use arcode_ui_kit::{
    ButtonStyle, IconName, Tooltip, button, icon, kbd, logo, mono_font_family, theme, truncated,
};
use gpui::{AnyView, Context, IntoElement, MouseButton, SharedString, div, prelude::*, px};

use crate::actions::{NewProject, OpenFolder, shortcut_label};
use crate::app_state;
use crate::workspace::Workspace;

/// Most recent projects listed.
const MAX_RECENT: usize = 8;
/// Width of the content block.
const CONTENT_W: f32 = 760.;
/// Width of the "Get going" column.
const SIDE_W: f32 = 250.;
/// Space between the two columns.
const COLUMN_GAP: f32 = 56.;

/// When a project was last opened, in words: "just now", "5 minutes ago",
/// "2 hours ago", "yesterday", "3 days ago", "last week", "3 weeks ago",
/// "last month", "4 months ago", "over a year ago". `then` and `now` are
/// seconds since the Unix epoch; a time in the future reads "just now".
pub fn relative_time(then: u64, now: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    let ago = now.saturating_sub(then);
    let plural = |n: u64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match ago {
        0..MINUTE => "just now".into(),
        MINUTE..HOUR => plural(ago / MINUTE, "minute"),
        HOUR..DAY => plural(ago / HOUR, "hour"),
        _ if ago < 2 * DAY => "yesterday".into(),
        _ if ago < 7 * DAY => plural(ago / DAY, "day"),
        _ if ago < 14 * DAY => "last week".into(),
        _ if ago < 30 * DAY => plural(ago / (7 * DAY), "week"),
        _ if ago < 60 * DAY => "last month".into(),
        _ if ago < 365 * DAY => plural(ago / (30 * DAY), "month"),
        _ => "over a year ago".into(),
    }
}

/// `path` with the home folder written as `~` (`~/code/demo`).
pub fn tilde_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// Seconds since the Unix epoch now.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Renders the welcome screen.
pub fn render(_ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let home = std::env::home_dir();
    let now = now_secs();
    let recent: Vec<(PathBuf, String, String, String)> = app_state::recent_projects(cx)
        .iter()
        .take(MAX_RECENT)
        .map(|p| {
            (
                p.path.clone(),
                p.name.clone(),
                tilde_path(&p.path, home.as_deref()),
                relative_time(p.last_opened, now),
            )
        })
        .collect();

    let heading = div()
        .flex()
        .items_center()
        .gap(px(14.))
        .child(logo(px(56.)))
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(scale.display)
                        .font_weight(WEIGHT_SEMIBOLD)
                        .text_color(c.text)
                        .child("Arcode"),
                )
                .child(
                    div()
                        .text_size(scale.body)
                        .text_color(c.text_muted)
                        .child("A fast, native code editor"),
                ),
        );

    // Open Folder is the one solid accent block on this screen.
    let actions = div()
        .flex()
        .gap_2()
        .child(
            button("welcome-open", "Open Folder…")
                .icon(IconName::FolderOpen)
                .style(ButtonStyle::Primary)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
        )
        .child(
            button("welcome-new", "New Project…")
                .icon(IconName::Plus)
                .style(ButtonStyle::Secondary)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(NewProject), cx)),
        );

    let section_title = |title: &'static str| {
        div()
            .text_size(scale.meta)
            .font_weight(WEIGHT_SEMIBOLD)
            .text_color(c.text)
            .child(title)
    };

    // A small icon button that shows while its row is hovered.
    let row_button = |id: (&'static str, usize), glyph: IconName, row: &SharedString| {
        let own = SharedString::from(format!("{}-{}", id.0, id.1));
        div()
            .id(id)
            .group(own.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(px(7.))
            .opacity(0.)
            .group_hover(row.clone(), |s| s.opacity(1.))
            .child(
                icon(glyph)
                    .size(icon_size::TOOLBAR)
                    .color(c.text_muted)
                    .group_hover_color(own, c.text),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    };

    let recent_list = if recent.is_empty() {
        div()
            .text_size(scale.body)
            .text_color(c.text_muted)
            .child("Projects you open will show up here.")
    } else {
        div()
            .flex()
            .flex_col()
            .mx(px(-10.))
            .children(
                recent
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (path, name, shown, when))| {
                        let group = SharedString::from(format!("recent-{ix}"));
                        let open_path = path.clone();
                        let window_path = path.clone();
                        div()
                            .id(("welcome-recent", ix))
                            .group(group.clone())
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .h(ROW_H_WELCOME)
                            .px(px(10.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(c.pill))
                            .child(
                                icon(IconName::Folder)
                                    .size(px(16.))
                                    .color(c.text_muted)
                                    .group_hover_color(group.clone(), c.accent),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(scale.body)
                                            .font_weight(WEIGHT_MEDIUM)
                                            .text_color(c.text)
                                            .overflow_hidden()
                                            .child(truncated(name)),
                                    )
                                    .child(
                                        div()
                                            .font_family(mono_font_family())
                                            .text_size(scale.micro)
                                            .text_color(c.text_muted)
                                            .overflow_hidden()
                                            .child(truncated(shown).middle()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(scale.meta)
                                    .text_color(c.text_muted)
                                    .child(when),
                            )
                            .child(
                                row_button(
                                    ("welcome-new-window", ix),
                                    IconName::ExternalLink,
                                    &group,
                                )
                                .tooltip(|_, cx| -> AnyView {
                                    Tooltip::text("Open in New Window", cx)
                                })
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    crate::windows::open_project_in_new_window(
                                        window_path.clone(),
                                        cx,
                                    );
                                }),
                            )
                            .child(
                                row_button(("welcome-remove", ix), IconName::X, &group)
                                    .tooltip(|_, cx| -> AnyView {
                                        Tooltip::text("Remove from Recent Projects", cx)
                                    })
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        app_state::remove_recent(&path, cx);
                                        cx.notify();
                                    })),
                            )
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.open_project(open_path.clone(), window, cx)
                            }))
                    }),
            )
    };

    let hint = |label: &'static str, keys: &str| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(32.))
            .border_b_1()
            .border_color(c.border)
            .text_size(scale.body)
            .text_color(c.text)
            .child(label)
            .child(kbd(shortcut_label(keys)))
    };
    let side = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(SIDE_W))
        .pt(px(96.))
        .child(div().pb(px(6.)).child(section_title("Get going")))
        .child(hint("Open folder", "secondary-o"))
        .child(hint("Find file", "secondary-p"))
        .child(hint("Commands", "secondary-shift-p"))
        .child(hint("New window", "secondary-shift-n"))
        .child(hint("Toggle panel", "secondary-j"))
        .child(hint("Settings", "secondary-,"))
        .child(
            div()
                .pt(px(18.))
                .text_size(scale.meta)
                .text_color(c.text_muted)
                .child("No account, no telemetry. Everything stays on this machine."),
        );

    let main = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(280.))
        .child(heading)
        .child(div().pt(px(28.)).child(actions))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(40.))
                .child(section_title("Recent projects"))
                .child(recent_list),
        );

    div()
        .id("welcome")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .justify_center()
        .child(
            div()
                .w(px(CONTENT_W))
                .max_w_full()
                .px_6()
                .pt(px(96.))
                .pb_8()
                .flex()
                .flex_wrap()
                .gap_x(px(COLUMN_GAP))
                .gap_y(px(8.))
                .child(main)
                .child(side),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_like_a_person_would_say_them() {
        let now = 1_000_000_000;
        let ago = |secs: u64| relative_time(now - secs, now);
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(5 * 60 + 30), "5 minutes ago");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(2 * 3600), "2 hours ago");
        assert_eq!(ago(30 * 3600), "yesterday");
        assert_eq!(ago(3 * 86400), "3 days ago");
        assert_eq!(ago(8 * 86400), "last week");
        assert_eq!(ago(21 * 86400), "3 weeks ago");
        assert_eq!(ago(45 * 86400), "last month");
        assert_eq!(ago(120 * 86400), "4 months ago");
        assert_eq!(ago(400 * 86400), "over a year ago");
        // A clock that went backwards, or a project never opened.
        assert_eq!(relative_time(now + 10, now), "just now");
        assert_eq!(relative_time(0, now), "over a year ago");
    }

    #[test]
    fn paths_under_home_start_with_a_tilde() {
        let home = Path::new("/home/ada");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            tilde_path(Path::new("/home/ada/code/demo"), Some(home)),
            format!("~{sep}code/demo")
        );
        assert_eq!(tilde_path(home, Some(home)), "~");
        assert_eq!(tilde_path(Path::new("/srv/demo"), Some(home)), "/srv/demo");
        assert_eq!(
            tilde_path(Path::new("/home/adam"), Some(home)),
            "/home/adam"
        );
        assert_eq!(tilde_path(Path::new("/x"), None), "/x");
    }
}
