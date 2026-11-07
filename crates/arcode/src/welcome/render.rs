//! Drawing the welcome screen (docs/DESIGN.md 6.15).

use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, icon_size, type_scale};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, input_frame, kbd, logo, theme};
use gpui::{Context, Div, Focusable, IntoElement, Window, div, prelude::*, px};

use super::Welcome;
use crate::actions::{CloneRepository, NewProject, OpenFolder, shortcut_label};

/// Width of the content block.
const CONTENT_W: f32 = 760.;
/// Width of the "Get going" column.
const SIDE_W: f32 = 250.;
/// Space between the two columns.
const COLUMN_GAP: f32 = 56.;
/// Width of the filter field.
const FILTER_W: f32 = 200.;

impl Welcome {
    /// The filter field at the top of the list.
    fn render_filter(&self, window: &Window, cx: &Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let focused = self.filter.focus_handle(cx).is_focused(window);
        input_frame(focused, false, cx)
            .w(px(FILTER_W))
            .flex_none()
            .child(
                icon(IconName::Search)
                    .size(icon_size::TOOLBAR)
                    .color(c.text_muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .overflow_hidden()
                    .child(self.filter.clone()),
            )
    }

    /// The recent projects: "Pinned" first, then "Recent projects", with
    /// the filter at the right of the first title.
    fn render_projects(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let title = |text: &'static str| {
            div()
                .flex_1()
                .text_size(scale.meta)
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(c.text)
                .child(text)
        };
        let (rows, visible) = self.rows(cx);
        if rows.is_empty() {
            return div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(title("Recent projects"))
                .child(
                    div()
                        .text_size(scale.body)
                        .text_color(c.text_muted)
                        .child("Projects you open will show up here."),
                );
        }
        let filtering = self.shows_filter(cx) && !self.filter_text(cx).trim().is_empty();
        let groups = if filtering {
            // One list, best match first, so Enter opens the top row.
            vec![("Recent projects", (0..visible.len()).collect())]
        } else {
            let (pinned, others): (Vec<usize>, Vec<usize>) =
                (0..visible.len()).partition(|&v| rows[visible[v]].pinned);
            vec![("Pinned", pinned), ("Recent projects", others)]
        };
        let first = groups.iter().position(|(_, g)| !g.is_empty()).unwrap_or(1);
        let mut out = div().flex().flex_col();
        for (g, (name, members)) in groups.into_iter().enumerate() {
            if members.is_empty() && g != first {
                continue;
            }
            let mut header = div()
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                .when(g != first, |el| el.mt(px(14.)))
                .child(title(name));
            if g == first && self.shows_filter(cx) {
                header = header.child(self.render_filter(window, cx));
            }
            out = out.child(header);
            let mut list = div().flex().flex_col().mx(px(-10.)).mt(px(4.));
            for v in members {
                list = list.child(self.render_row(v, rows[visible[v]].clone(), cx));
            }
            out = out.child(list);
        }
        if visible.is_empty() {
            out = out.child(
                div()
                    .pt(px(4.))
                    .text_size(scale.body)
                    .text_color(c.text_muted)
                    .child(format!(
                        "No projects match \u{201c}{}\u{201d}.",
                        self.filter_text(cx).trim()
                    )),
            );
        }
        out
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_checked(cx);
        // The filter field went away (the list got short): its keys too.
        if !self.shows_filter(cx) && self.filter.focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle);
        }
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);

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
            .flex_wrap()
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
            )
            .child(
                button("welcome-clone", "Clone Repository…")
                    .icon(IconName::GitBranch)
                    .style(ButtonStyle::Secondary)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(CloneRepository), cx)
                    }),
            );

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
            .child(
                div()
                    .pb(px(6.))
                    .text_size(scale.meta)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .text_color(c.text)
                    .child("Get going"),
            )
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
                    .child("No account, no telemetry. Nothing leaves this machine unless you ask."),
            );

        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(280.))
            .child(heading)
            .child(div().pt(px(28.)).child(actions))
            .child(div().pt(px(40.)).child(self.render_projects(window, cx)));

        div()
            .id("welcome")
            .key_context(super::CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::clear_filter))
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .justify_center()
            // Not stretched to the window's height: a long list makes the
            // content taller than the screen, which then scrolls.
            .items_start()
            // The margin sits outside the 760 content block (DESIGN.md 6.15).
            .px_6()
            .child(
                div()
                    .w(px(CONTENT_W))
                    .max_w_full()
                    .pt(px(96.))
                    .pb_8()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(COLUMN_GAP))
                    .gap_y(px(8.))
                    .child(main)
                    .child(side),
            )
            .children(self.render_menu(cx))
    }
}
