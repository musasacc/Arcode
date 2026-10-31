//! The breakpoints and watch lists of the Run and Debug sidebar
//! (docs/DESIGN.md 6.9).

use std::collections::HashMap;
use std::path::PathBuf;

use arcode_ui_kit::metrics::type_scale;
use arcode_ui_kit::{
    IconName, Tooltip, border_width, icon, icon_button, list_row, readout, spacer, theme, truncated,
};
use gpui::{AnyView, App, Context, SharedString, div, prelude::*, px};

use super::super::panel::WATCH_CONTEXT;
use super::super::panel::changes::change_key;
use super::super::panel::tree::{render_row, watch_rows};
use super::{DebugSidebar, DebugSidebarEvent};
use crate::pane::breadcrumb::{Symbol, symbols_of};

/// The innermost symbol around `line` in `symbols` (`fn main`).
pub fn enclosing_symbol(symbols: &[Symbol], line: usize) -> Option<&str> {
    symbols
        .iter()
        .filter(|s| s.lines.contains(&line))
        .min_by_key(|s| s.lines.len())
        .map(|s| s.label.as_str())
}

impl DebugSidebar {
    /// The function around each breakpoint's line, from the outlines of
    /// open files (cached per buffer version).
    pub(super) fn breakpoint_symbols(&mut self, cx: &App) -> HashMap<PathBuf, Vec<Symbol>> {
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

    pub(super) fn render_breakpoints(&mut self, cx: &mut Context<Self>) -> gpui::Div {
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
                .map_or_else(
                    || bp.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
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
                    .bg(if problem.is_some() {
                        c.warning
                    } else {
                        c.error
                    })
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
                            .flex()
                            .flex_shrink()
                            .min_w(px(24.))
                            .max_w(px(140.))
                            .overflow_hidden()
                            .text_color(if !enabled {
                                c.text_muted
                            } else if problem.is_some() {
                                c.warning
                            } else {
                                c.text
                            })
                            .child(truncated(file)),
                    )
                    .when_some(suffix, |el, suffix| {
                        el.child(
                            div()
                                .flex()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(scale.meta)
                                .text_color(c.text_muted)
                                .child(truncated(suffix)),
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

    pub(super) fn render_watches(&mut self, cx: &mut Context<Self>) -> gpui::Div {
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
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(px(24.))
                            .child(self.watch_input.clone()),
                    ),
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
