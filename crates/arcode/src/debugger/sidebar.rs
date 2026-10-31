//! The Run and Debug sidebar view (docs/DESIGN.md 6.9): the launch
//! configuration (pick one, start, stop, restart, open `launch.toml`),
//! the breakpoints list (check to enable, click to open, x to remove) and
//! the watch expressions.

use std::collections::HashMap;
use std::path::PathBuf;

use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_RADIUS, type_scale};
use arcode_ui_kit::{
    IconName, Tooltip, badge, icon, icon_button, list_row, panel_title, section_header, spacer,
    theme, truncated,
};
use gpui::{
    AnyView, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    IntoElement, SharedString, Subscription, Window, div, prelude::*, px,
};

use super::Debugger;
use super::actions::{OpenLaunchConfig, RestartDebugging, StartOrContinue, StopDebugging};
use super::panel::AddWatch;
use super::panel::changes::ValueChanges;
use crate::actions::shortcut_label;
use crate::pane::breadcrumb::Symbol;
use crate::text_field::TextField;

#[path = "sidebar_lists.rs"]
mod lists;

pub use lists::enclosing_symbol;

/// What the view asks the workspace to do.
pub enum DebugSidebarEvent {
    /// Open a breakpoint's line.
    Open(PathBuf, usize),
}

/// The view.
pub struct DebugSidebar {
    debugger: Entity<Debugger>,
    focus_handle: FocusHandle,
    /// The configuration list is open.
    choices_open: bool,
    /// The watch input is shown.
    adding_watch: bool,
    watch_input: Entity<TextField>,
    /// Watch values that changed since the previous stop.
    changes: ValueChanges,
    /// Outlines of open files with breakpoints (for the function names),
    /// by buffer version.
    symbols: HashMap<PathBuf, (u64, Vec<Symbol>)>,
    _sub: Subscription,
}

impl EventEmitter<DebugSidebarEvent> for DebugSidebar {}

impl Focusable for DebugSidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// What an outlined tool does when clicked.
type ToolClick = Box<dyn Fn(&mut Window, &mut App)>;

/// A 28 px outlined square button (stop, restart).
fn outlined_tool(
    id: &'static str,
    glyph: IconName,
    glyph_size: f32,
    color: Hsla,
    tip: String,
    on_click: Option<ToolClick>,
    cx: &App,
) -> impl IntoElement {
    let c = theme(cx).colors.clone();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(CONTROL_H)
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(c.border_strong)
        .when(on_click.is_some(), |el| {
            el.cursor_pointer().hover(|s| s.bg(c.hover))
        })
        .child(icon(glyph).size(px(glyph_size)).color(color))
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tip.clone(), cx) })
        .when_some(on_click, |el, f| {
            el.on_click(move |_, window, cx| f(window, cx))
        })
}

impl DebugSidebar {
    /// A view of `debugger`.
    pub fn new(debugger: Entity<Debugger>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&debugger, |_, _, cx| cx.notify());
        Self {
            debugger,
            focus_handle: cx.focus_handle(),
            choices_open: false,
            adding_watch: false,
            watch_input: cx.new(|cx| TextField::new("Watch an expression", cx)),
            changes: ValueChanges::default(),
            symbols: HashMap::new(),
            _sub: sub,
        }
    }

    fn add_watch(&mut self, _: &AddWatch, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.watch_input.read(cx).text().trim().to_string();
        self.watch_input.update(cx, |f, cx| f.set_text("", cx));
        self.adding_watch = false;
        window.focus(&self.focus_handle);
        if !text.is_empty() {
            self.debugger.update(cx, |d, cx| d.add_watch(text, cx));
        }
        cx.notify();
    }

    fn render_configs(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let d = self.debugger.read(cx);
        let active = d.is_active();
        let stopped = d.session().is_some_and(|s| s.is_stopped());
        let name: SharedString = d
            .choices()
            .get(d.selected())
            .map_or_else(|| "No configuration".into(), |c| c.name.clone().into());
        let dropdown = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(CONTROL_H)
            .rounded(CONTROL_RADIUS)
            .border_1()
            .border_color(c.border_strong)
            .child(
                icon_button("debug-start", IconName::CirclePlay)
                    .size(px(26.))
                    .icon_color(c.success)
                    .tooltip(if stopped {
                        format!("Continue ({})", shortcut_label("f5"))
                    } else {
                        format!(
                            "Start the selected configuration ({})",
                            shortcut_label("f5")
                        )
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(StartOrContinue), cx)
                    }),
            )
            .child(
                div()
                    .id("debug-choice")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap(px(4.))
                    .pr(px(8.))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(truncated(name)),
                    )
                    .child(
                        icon(IconName::ChevronDown)
                            .size(px(14.))
                            .color(c.text_muted),
                    )
                    .tooltip(|_, cx| -> AnyView { Tooltip::text("Choose a configuration", cx) })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.choices_open = !view.choices_open;
                        cx.notify();
                    })),
            );
        let stop: Option<ToolClick> = active.then(|| {
            Box::new(|window: &mut Window, cx: &mut App| {
                window.dispatch_action(Box::new(StopDebugging), cx)
            }) as ToolClick
        });
        let restart: ToolClick =
            Box::new(|window, cx| window.dispatch_action(Box::new(RestartDebugging), cx));
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(8.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(dropdown)
                    .child(outlined_tool(
                        "debug-stop",
                        IconName::Square,
                        12.,
                        if active { c.error } else { c.text_disabled },
                        format!("Stop debugging ({})", shortcut_label("shift-f5")),
                        stop,
                        cx,
                    ))
                    .child(outlined_tool(
                        "debug-restart",
                        IconName::RotateCcw,
                        14.,
                        c.text_muted,
                        format!("Restart ({})", shortcut_label("secondary-shift-f5")),
                        Some(restart),
                        cx,
                    )),
            );
        if self.choices_open {
            let selected = d.selected();
            for (ix, choice) in d.choices().iter().enumerate() {
                let source = if choice.is_from_file() {
                    "launch.toml"
                } else {
                    "detected"
                };
                col = col.child(
                    list_row(("launch-choice", ix), ix == selected, cx)
                        .mx(px(0.))
                        .child(icon(IconName::Bug).size(px(14.)).color(if ix == selected {
                            c.accent
                        } else {
                            c.text_muted
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(truncated(choice.name.clone())),
                        )
                        .child(
                            div()
                                .text_size(scale.meta)
                                .text_color(c.text_muted)
                                .child(source),
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.choices_open = false;
                            view.debugger.update(cx, |d, cx| d.select(ix, cx));
                        })),
                );
            }
        }
        if d.choices().is_empty() {
            col = col.child(
                div()
                    .px(px(6.))
                    .text_size(scale.meta)
                    .text_color(c.text_muted)
                    .child("No launch configurations found. Add one to .arcode/launch.toml, or use the run buttons next to main and tests."),
            );
        }
        for warning in d.warnings() {
            col = col.child(
                div()
                    .px(px(6.))
                    .text_size(scale.meta)
                    .text_color(c.warning)
                    .child(SharedString::from(warning.clone())),
            );
        }
        col
    }
}

impl Render for DebugSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let breakpoints = d.book().all().len();
        let watches = d.watches().len();
        let configs = self.render_configs(cx);
        let breakpoint_list = self.render_breakpoints(cx);
        let watch_list = self.render_watches(cx);
        div()
            .id("debug-sidebar")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .text_size(type_scale(cx).body)
            .child(
                panel_title("Run and debug", "", cx).child(
                    icon_button("open-launch", IconName::Settings)
                        .size(px(24.))
                        .tooltip("Open (or create) .arcode/launch.toml")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenLaunchConfig), cx)
                        }),
                ),
            )
            .child(configs)
            .child(div().flex_none().h(px(1.)).bg(c.border))
            .child(
                section_header("Breakpoints", cx)
                    .pl(px(14.))
                    .pr(px(8.))
                    .child(badge(breakpoints.to_string()))
                    .child(spacer())
                    .when(breakpoints > 0, |el| {
                        el.child(
                            icon_button("bp-remove-all", IconName::Trash)
                                .size(px(24.))
                                .tooltip("Remove all breakpoints")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.debugger
                                        .update(cx, |d, cx| d.remove_all_breakpoints(cx));
                                })),
                        )
                    }),
            )
            .child(breakpoint_list)
            .child(
                section_header("Watch", cx)
                    .mt(px(8.))
                    .pl(px(14.))
                    .pr(px(8.))
                    .child(badge(watches.to_string()))
                    .child(spacer())
                    .child(
                        icon_button("watch-add", IconName::Plus)
                            .size(px(24.))
                            .tooltip("Watch an expression")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.adding_watch = true;
                                window.focus(&view.watch_input.focus_handle(cx));
                                cx.notify();
                            })),
                    ),
            )
            .child(watch_list)
            .child(div().flex_none().h(px(8.)))
    }
}
