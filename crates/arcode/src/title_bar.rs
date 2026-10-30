//! The title bar: project dropdown, git branch, file search box, Run and
//! settings buttons.

use arcode_ui_kit::{
    ButtonStyle, IconName, Tooltip, button, icon, icon_button, kbd, theme, truncated,
};
use gpui::{AnyView, Context, IntoElement, SharedString, Window, div, prelude::*, px};

use crate::actions::{OpenSettings, RunProject, ToggleFinder, shortcut_label};
use crate::workspace::Workspace;

/// Height of the title bar.
pub const TITLE_BAR_HEIGHT: f32 = 36.;

/// Renders the title bar for `ws`.
pub fn render(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let project = ws.project();
    let name: SharedString = project
        .map(|p| p.name.clone().into())
        .unwrap_or_else(|| "No Project".into());
    let branch = ws.branch_label(cx);
    let run_command = project.and_then(|p| p.run_command());
    let menu_open = ws.project_menu_open();

    let dropdown = div()
        .id("project-dropdown")
        .flex()
        .items_center()
        .gap_1p5()
        .h(px(26.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|s| s.bg(c.hover))
        .when(menu_open, |el| el.bg(c.hover))
        .child(icon(IconName::Folder).size(px(14.)).color(c.accent))
        .child(
            div()
                .max_w(px(220.))
                .min_w_0()
                .overflow_hidden()
                .child(truncated(name)),
        )
        .child(
            icon(IconName::ChevronDown)
                .size(px(12.))
                .color(c.text_muted),
        )
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_project_menu(cx)));

    let left = div()
        .flex()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .items_center()
        .gap_1()
        .child(dropdown)
        .when_some(branch, |el, branch| {
            el.child(
                div()
                    .id("branch")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .min_w_0()
                    .h(px(26.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .text_color(c.text_muted)
                    .child(icon(IconName::GitBranch).size(px(14.)))
                    .child(div().min_w_0().overflow_hidden().child(truncated(branch)))
                    .tooltip(|_, cx| -> AnyView { Tooltip::text("Switch branch", cx) })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::SwitchBranch), cx)
                    }),
            )
        });

    let search = div()
        .id("search-box")
        .flex()
        .items_center()
        .gap_2()
        .w(px(380.))
        .min_w(px(160.))
        .h(px(26.))
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(c.border)
        .bg(c.surface)
        .text_color(c.text_muted)
        .cursor_pointer()
        .hover(|s| s.border_color(c.border_focused))
        .child(icon(IconName::Search).size(px(14.)))
        .child(div().flex_1().child("Search files"))
        .child(kbd(shortcut_label("secondary-p")))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleFinder), cx));

    let run_tooltip = match run_command {
        Some(cmd) => format!("Run `{cmd}` (F5)"),
        None => "No run command detected".to_string(),
    };
    let right = div()
        .flex()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .items_center()
        .justify_end()
        .gap_1()
        .when(project.is_some(), |el| {
            el.child(
                button("run", "Run")
                    .icon(IconName::Play)
                    .style(ButtonStyle::Ghost)
                    .tooltip(run_tooltip)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(RunProject), cx)),
            )
        })
        .child(
            icon_button("settings", IconName::Settings)
                .tooltip(format!("Settings ({})", shortcut_label("secondary-,")))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
        );

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_3()
        .h(px(TITLE_BAR_HEIGHT))
        .px_2()
        .bg(c.title_bar)
        .border_b_1()
        .border_color(c.border)
        .text_sm()
        .child(left)
        .child(search)
        .child(right)
}
