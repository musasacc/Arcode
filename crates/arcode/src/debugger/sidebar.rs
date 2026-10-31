//! The Run and Debug sidebar view: launch configurations (pick one, start
//! it, open `launch.toml`) and the breakpoints list (check to enable,
//! click to open, x to remove).

use std::path::PathBuf;

use arcode_ui_kit::{ButtonStyle, IconName, Tooltip, button, icon, icon_button, theme};
use gpui::{
    AnyView, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use super::Debugger;
use super::actions::{OpenLaunchConfig, StartOrContinue, StopDebugging};
use crate::actions::shortcut_label;
use crate::project::relative_display;

/// What the view asks the workspace to do.
pub enum DebugSidebarEvent {
    /// Open a breakpoint's line.
    Open(PathBuf, usize),
}

/// The view.
pub struct DebugSidebar {
    debugger: Entity<Debugger>,
    focus_handle: FocusHandle,
    _sub: Subscription,
}

impl EventEmitter<DebugSidebarEvent> for DebugSidebar {}

impl Focusable for DebugSidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DebugSidebar {
    /// A view of `debugger`.
    pub fn new(debugger: Entity<Debugger>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&debugger, |_, _, cx| cx.notify());
        Self {
            debugger,
            focus_handle: cx.focus_handle(),
            _sub: sub,
        }
    }

    fn header(title: &'static str, cx: &App) -> gpui::Div {
        let c = theme(cx).colors.clone();
        div()
            .flex()
            .items_center()
            .h(px(28.))
            .px_3()
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(c.text_muted)
            .child(div().flex_1().child(title))
    }

    fn render_configs(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let selected = d.selected();
        let active = d.is_active();
        let mut list = div().flex().flex_col();
        if d.choices().is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child("No launch configurations found. Add one to .arcode/launch.toml, or use the run buttons next to main and tests."),
            );
        }
        for (ix, choice) in d.choices().iter().enumerate() {
            let is_selected = ix == selected;
            let source = if choice.is_from_file() {
                "launch.toml"
            } else {
                "detected"
            };
            list = list.child(
                div()
                    .id(("launch-choice", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(24.))
                    .px_3()
                    .cursor_pointer()
                    .when(is_selected, |el| el.bg(c.selection))
                    .hover(|s| s.bg(c.hover))
                    .child(
                        icon(if is_selected {
                            IconName::CircleDot
                        } else {
                            IconName::Bug
                        })
                        .size(px(13.))
                        .color(if is_selected {
                            c.accent
                        } else {
                            c.text_muted
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(choice.name.clone())),
                    )
                    .child(div().text_xs().text_color(c.text_disabled).child(source))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.debugger.update(cx, |d, cx| d.select(ix, cx));
                    })),
            );
        }
        for warning in d.warnings() {
            list = list.child(
                div()
                    .px_3()
                    .py_0p5()
                    .text_xs()
                    .text_color(c.warning)
                    .child(SharedString::from(warning.clone())),
            );
        }
        let start = if active {
            button("debug-stop", "Stop")
                .icon(IconName::Square)
                .style(ButtonStyle::Secondary)
                .tooltip(format!("Stop debugging ({})", shortcut_label("shift-f5")))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(StopDebugging), cx))
        } else {
            button("debug-start", "Start Debugging")
                .icon(IconName::Play)
                .style(ButtonStyle::Primary)
                .tooltip(format!(
                    "Start the selected configuration ({})",
                    shortcut_label("f5")
                ))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(StartOrContinue), cx))
        };
        list.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(start)
                .child(
                    icon_button("open-launch", IconName::Settings)
                        .size(px(26.))
                        .tooltip("Open (or create) .arcode/launch.toml")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenLaunchConfig), cx)
                        }),
                ),
        )
    }

    fn render_breakpoints(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let root = d.root().map(|r| r.to_path_buf());
        let mut list = div().flex().flex_col();
        let all = d.book().all();
        if all.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child("Click left of a line number (or press F9) to add a breakpoint. Right-click it for a condition, hit count or log message."),
            );
        }
        for bp in all {
            let id = bp.id;
            let enabled = bp.enabled;
            let file = match &root {
                Some(root) => relative_display(root, &bp.path),
                None => bp.path.display().to_string(),
            };
            let label: SharedString = format!("{file}:{}", bp.line + 1).into();
            let detail = bp.detail();
            let problem = (bp.verified == Some(false)).then(|| {
                bp.message
                    .clone()
                    .unwrap_or_else(|| "Not set by the debug adapter".into())
            });
            let (path, line) = (bp.path.clone(), bp.line);
            list = list.child(
                div()
                    .id(("bp-row", id))
                    .group("bp-row")
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(24.))
                    .py_0p5()
                    .px_3()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .child(
                        div()
                            .id(("bp-check", id))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(14.))
                            .rounded_sm()
                            .border_1()
                            .border_color(if enabled { c.accent } else { c.border })
                            .when(enabled, |el| el.bg(c.accent))
                            .when(enabled, |el| {
                                el.child(icon(IconName::Check).size(px(11.)).color(c.accent_text))
                            })
                            .tooltip(|_, cx| -> AnyView { Tooltip::text("Enable or disable", cx) })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                view.debugger
                                    .update(cx, |d, cx| d.set_breakpoint_enabled(id, !enabled, cx));
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(if enabled { c.text } else { c.text_muted })
                                    .child(label),
                            )
                            .when_some(detail.or(problem.clone()), |el, detail| {
                                el.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(if problem.is_some() {
                                            c.warning
                                        } else {
                                            c.text_muted
                                        })
                                        .child(SharedString::from(detail)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .opacity(0.)
                            .group_hover("bp-row", |s| s.opacity(1.))
                            .child(
                                icon_button(("bp-remove", id), IconName::X)
                                    .size(px(18.))
                                    .tooltip("Remove breakpoint")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        cx.stop_propagation();
                                        view.debugger
                                            .update(cx, |d, cx| d.remove_breakpoint(id, cx));
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(DebugSidebarEvent::Open(path.clone(), line));
                    })),
            );
        }
        list
    }
}

impl Render for DebugSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let has_breakpoints = !self.debugger.read(cx).book().is_empty();
        div()
            .id("debug-sidebar")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .px_3()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(c.text_muted)
                    .child("RUN AND DEBUG"),
            )
            .child(self.render_configs(cx))
            .child(
                Self::header("BREAKPOINTS", cx)
                    .border_t_1()
                    .border_color(c.border)
                    .when(has_breakpoints, |el| {
                        el.child(
                            icon_button("bp-remove-all", IconName::Trash)
                                .size(px(20.))
                                .tooltip("Remove all breakpoints")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.debugger
                                        .update(cx, |d, cx| d.remove_all_breakpoints(cx));
                                })),
                        )
                    }),
            )
            .child(self.render_breakpoints(cx))
    }
}
