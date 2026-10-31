//! The Run and Debug sidebar view (docs/DESIGN.md 6.9): the launch
//! configuration (pick one, start, stop, restart, open `launch.toml`),
//! the breakpoints list (check to enable, click to open, x to remove) and
//! the watch expressions.

use std::collections::HashMap;
use std::path::PathBuf;

use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_RADIUS, type_scale};
use arcode_ui_kit::{
    IconName, Tooltip, badge, border_width, icon, icon_button, list_row, panel_title, readout,
    section_header, spacer, theme, truncated,
};
use gpui::{
    AnyView, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    IntoElement, SharedString, Subscription, Window, div, prelude::*, px,
};

use super::Debugger;
use super::actions::{OpenLaunchConfig, RestartDebugging, StartOrContinue, StopDebugging};
use super::panel::changes::{ValueChanges, change_key};
use super::panel::tree::{render_row, watch_rows};
use super::panel::{AddWatch, WATCH_CONTEXT};
use crate::actions::shortcut_label;
use crate::pane::breadcrumb::{Symbol, symbols_of};
use crate::text_field::TextField;

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

/// The innermost symbol around `line` in `symbols` (`fn main`).
pub fn enclosing_symbol(symbols: &[Symbol], line: usize) -> Option<&str> {
    symbols
        .iter()
        .filter(|s| s.lines.contains(&line))
        .min_by_key(|s| s.lines.len())
        .map(|s| s.label.as_str())
}

/// A 28 px outlined square button (stop, restart).
fn outlined_tool(
    id: &'static str,
    glyph: IconName,
    glyph_size: f32,
    color: Hsla,
    tip: String,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
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

    /// The function around each breakpoint's line, from the outlines of
    /// open files (cached per buffer version).
    fn breakpoint_symbols(&mut self, cx: &App) -> HashMap<PathBuf, Vec<Symbol>> {
        let d = self.debugger.read(cx);
        let files = d.book().files();
        let mut out = HashMap::new();
        for (editor, path) in d.open_editors(cx) {
            if !files.contains(&path) {
                continue;
            }
            let e = editor.read(cx);
            let version = e.buffer().version();
            let fresh = self.symbols.get(&path).is_some_and(|(v, _)| *v == version);
            if !fresh {
                let symbols = if e.is_large_file() {
                    Vec::new()
                } else {
                    symbols_of(&e.outline(), e.language().id == "rust")
                };
                self.symbols.insert(path.clone(), (version, symbols));
            }
            if let Some((_, symbols)) = self.symbols.get(&path) {
                out.insert(path, symbols.clone());
            }
        }
        self.symbols.retain(|path, _| files.contains(path));
        out
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
                        format!("Start the selected configuration ({})", shortcut_label("f5"))
                    })
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(StartOrContinue), cx)),
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
                    .child(div().flex_1().min_w_0().overflow_hidden().child(truncated(name)))
                    .child(icon(IconName::ChevronDown).size(px(14.)).color(c.text_muted))
                    .tooltip(|_, cx| -> AnyView { Tooltip::text("Choose a configuration", cx) })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.choices_open = !view.choices_open;
                        cx.notify();
                    })),
            );
        let stop: Option<Box<dyn Fn(&mut Window, &mut App)>> = active.then(|| {
            Box::new(|window: &mut Window, cx: &mut App| {
                window.dispatch_action(Box::new(StopDebugging), cx)
            }) as Box<dyn Fn(&mut Window, &mut App)>
        });
        let restart: Box<dyn Fn(&mut Window, &mut App)> = Box::new(|window, cx| {
            window.dispatch_action(Box::new(RestartDebugging), cx)
        });
        let mut col = div().flex().flex_col().gap(px(4.)).px(px(8.)).pb(px(10.)).child(
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
                        .child(
                            icon(IconName::Bug)
                                .size(px(14.))
                                .color(if ix == selected { c.accent } else { c.text_muted }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(choice.name.clone())),
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

    fn render_breakpoints(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let symbols = self.breakpoint_symbols(cx);
        let d = self.debugger.read(cx);
        let mut list = div().flex().flex_col();
        let all = d.book().all();
        if all.is_empty() {
            list = list.child(
                div()
                    .px(px(14.))
                    .pb(px(4.))
                    .text_size(scale.meta)
                    .text_color(c.text_muted)
                    .child("Click left of a line number (or press F9) to add a breakpoint. Right-click it for a condition, hit count or log message."),
            );
        }
        for bp in all {
            let id = bp.id;
            let enabled = bp.enabled;
            let file: SharedString = bp
                .path
                .file_name()
                .map_or_else(|| bp.path.display().to_string(), |n| n.to_string_lossy().into_owned())
                .into();
            let function = symbols
                .get(&bp.path)
                .and_then(|s| enclosing_symbol(s, bp.line))
                .map(str::to_string);
            let problem = (bp.verified == Some(false)).then(|| {
                bp.message
                    .clone()
                    .unwrap_or_else(|| "Not set by the debug adapter".into())
            });
            let suffix = bp.detail().or(function);
            let relative = match d.root() {
                Some(root) => crate::project::relative_display(root, &bp.path),
                None => bp.path.display().to_string(),
            };
            let tip: SharedString = match &problem {
                Some(problem) => format!("{relative}:{}\n{problem}", bp.line + 1).into(),
                None => format!("{relative}:{}", bp.line + 1).into(),
            };
            let (path, line) = (bp.path.clone(), bp.line);
            let group = SharedString::from(format!("bp-row-{id}"));
            let check = div()
                .id(("bp-check", id))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(14.))
                .rounded(px(4.))
                .when(enabled, |el| {
                    el.bg(c.accent)
                        .child(icon(IconName::Check).size(px(11.)).color(c.accent_text))
                })
                .when(!enabled, |el| el.border_1().border_color(c.border_strong))
                .tooltip(|_, cx| -> AnyView { Tooltip::text("Enable or disable", cx) })
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.debugger
                        .update(cx, |d, cx| d.set_breakpoint_enabled(id, !enabled, cx));
                }));
            let dot = if enabled {
                div()
                    .flex_none()
                    .size(px(8.))
                    .rounded_full()
                    .bg(if problem.is_some() { c.warning } else { c.error })
            } else {
                border_width(
                    div()
                        .flex_none()
                        .size(px(8.))
                        .rounded_full()
                        .border_color(c.text_muted),
                    px(1.5),
                )
            };
            list = list.child(
                list_row(("bp-row", id), false, cx)
                    .group(group.clone())
                    .h(px(26.))
                    .gap(px(8.))
                    .child(check)
                    .child(dot)
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(140.))
                            .truncate()
                            .text_color(if !enabled {
                                c.text_muted
                            } else if problem.is_some() {
                                c.warning
                            } else {
                                c.text
                            })
                            .child(file),
                    )
                    .when_some(suffix, |el, suffix| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(scale.meta)
                                .text_color(c.text_muted)
                                .child(SharedString::from(suffix)),
                        )
                    })
                    .child(spacer())
                    .child(
                        div()
                            .flex_none()
                            .group_hover(group.clone(), |s| s.invisible())
                            .child(readout((bp.line + 1).to_string())),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(4.))
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .child(
                                icon_button(("bp-remove", id), IconName::X)
                                    .size(px(20.))
                                    .tooltip("Remove breakpoint")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        cx.stop_propagation();
                                        view.debugger
                                            .update(cx, |d, cx| d.remove_breakpoint(id, cx));
                                    })),
                            ),
                    )
                    .relative()
                    .tooltip(move |_, cx| -> AnyView { Tooltip::text(tip.clone(), cx) })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(DebugSidebarEvent::Open(path.clone(), line));
                    })),
            );
        }
        list
    }

    fn render_watches(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let rows = watch_rows(d);
        let session = d.session();
        let stopped = session.is_some_and(|s| s.is_stopped());
        let generation = session.map_or(0, |s| s.generation);
        let frame = session
            .and_then(|s| s.selected_frame())
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let debugger = self.debugger.clone();
        let mut list = div().flex().flex_col();
        for (ix, row) in rows.into_iter().enumerate() {
            let changed = match &row.value {
                Some(value) if stopped && !row.error => {
                    self.changes
                        .observe(generation, &change_key(&frame, &row.path), value)
                }
                _ => false,
            };
            list = list.child(render_row(("watch-row", ix), row, changed, &debugger, cx));
        }
        if self.adding_watch {
            list = list.child(
                div()
                    .key_context(WATCH_CONTEXT)
                    .on_action(cx.listener(Self::add_watch))
                    .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            view.adding_watch = false;
                            window.focus(&view.focus_handle);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .mx(px(8.))
                    .my(px(2.))
                    .px(px(8.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(c.accent)
                    .child(icon(IconName::ChevronRight).size(px(13.)).color(c.accent))
                    .child(div().flex_1().min_w_0().h_full().child(self.watch_input.clone())),
            );
        } else if d.watches().is_empty() {
            list = list.child(
                div()
                    .px(px(14.))
                    .text_size(type_scale(cx).meta)
                    .text_color(c.text_muted)
                    .child("Add an expression to see its value at every stop."),
            );
        }
        list
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_symbol_names_a_breakpoint() {
        let sym = |lines: std::ops::Range<usize>, label: &str| Symbol {
            lines,
            label: label.to_string(),
        };
        let symbols = [
            sym(0..20, "impl Rect"),
            sym(2..6, "fn area"),
            sym(8..12, "fn perimeter"),
        ];
        assert_eq!(enclosing_symbol(&symbols, 3), Some("fn area"));
        assert_eq!(enclosing_symbol(&symbols, 7), Some("impl Rect"));
        assert_eq!(enclosing_symbol(&symbols, 30), None);
    }
}
