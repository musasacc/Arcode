//! The variables and watch trees: rows built from the debugger's state
//! (scopes of the selected frame and watch expressions, children loaded
//! lazily) and drawn as 22 px mono rows (docs/DESIGN.md 6.8, 6.9). Shared
//! by the Debug panel's Variables column and the sidebar's Watch section.

use arcode_dap::Variable;
use arcode_ui_kit::metrics::{ROW_H_DENSE, ROW_INSET, ROW_RADIUS, WEIGHT_MEDIUM, type_scale};
use arcode_ui_kit::{
    IconName, Tooltip, icon, icon_button, mono_font_family, theme, unsaved_ring,
};
use gpui::{AnyElement, App, ElementId, Entity, SharedString, div, prelude::*, px};

use super::value_style::{strip_paths, styled_value};
use crate::debugger::Debugger;
use crate::debugger::inspect::{scope_open_by_default, scope_path, watch_path};
use crate::debugger::session::{NodePath, WatchValue, child_path};

/// Deepest nesting shown (adapters can report cyclic structures).
const MAX_DEPTH: usize = 16;
/// Most children listed under one node.
const MAX_CHILDREN: usize = 1000;

/// One row of a tree.
pub struct Row {
    pub depth: usize,
    pub path: NodePath,
    pub reference: i64,
    pub default_open: bool,
    pub name: SharedString,
    pub value: Option<SharedString>,
    pub type_: Option<SharedString>,
    pub error: bool,
    /// A scope header (`Locals`), drawn in the UI font.
    pub scope: bool,
    /// A note such as `Loading…` (no value, not clickable).
    pub note: bool,
    /// Watch index (shows a remove button).
    pub watch: Option<usize>,
}

impl Row {
    fn note(depth: usize, text: &str) -> Row {
        Row {
            depth,
            path: String::new(),
            reference: 0,
            default_open: false,
            name: text.to_string().into(),
            value: None,
            type_: None,
            error: false,
            scope: false,
            note: true,
            watch: None,
        }
    }

    fn variable(v: &Variable, depth: usize, path: NodePath) -> Row {
        Row {
            depth,
            path,
            reference: v.variables_reference,
            default_open: false,
            name: v.name.clone().into(),
            value: Some(one_line(&v.value).into()),
            type_: v.type_.clone().map(Into::into),
            error: false,
            scope: false,
            note: false,
            watch: None,
        }
    }
}

/// A multi-line value on one line.
pub fn one_line(text: &str) -> String {
    text.split('\n').map(str::trim_end).collect::<Vec<_>>().join(" ")
}

/// The first non-empty line of an adapter's message (errors come with
/// source excerpts under them).
pub fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn push_children(d: &Debugger, rows: &mut Vec<Row>, reference: i64, path: &str, depth: usize) {
    let Some(s) = d.session() else {
        return;
    };
    let Some(children) = s.children.get(&reference) else {
        rows.push(Row::note(depth, "Loading…"));
        return;
    };
    if children.is_empty() {
        rows.push(Row::note(depth, "(empty)"));
    }
    for v in children.iter().take(MAX_CHILDREN) {
        let p = child_path(path, &v.name);
        let open = v.variables_reference > 0 && d.is_open(&p, false);
        rows.push(Row::variable(v, depth, p.clone()));
        if open && depth < MAX_DEPTH {
            push_children(d, rows, v.variables_reference, &p, depth + 1);
        }
    }
}

/// The scopes of the selected frame with their open children.
pub fn variable_rows(d: &Debugger) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(s) = d.session() else {
        return rows;
    };
    for scope in &s.scopes {
        let path = scope_path(scope);
        let default_open = scope_open_by_default(scope);
        let open = d.is_open(&path, default_open);
        rows.push(Row {
            depth: 0,
            path: path.clone(),
            reference: scope.variables_reference,
            default_open,
            name: scope.name.clone().into(),
            value: None,
            type_: None,
            error: false,
            scope: true,
            note: false,
            watch: None,
        });
        if open {
            push_children(d, &mut rows, scope.variables_reference, &path, 1);
        }
    }
    rows
}

/// The watch expressions with their open children.
pub fn watch_rows(d: &Debugger) -> Vec<Row> {
    let mut rows = Vec::new();
    for (ix, (expr, value)) in d.watches().iter().enumerate() {
        let path = watch_path(ix);
        let (value, type_, reference, error) = match value {
            WatchValue::Pending => (None, None, 0, false),
            WatchValue::Value {
                value,
                type_,
                reference,
            } => (Some(one_line(value)), type_.clone(), *reference, false),
            WatchValue::Error(e) => (Some(first_line(e)), None, 0, true),
        };
        let open = reference > 0 && d.is_open(&path, false);
        rows.push(Row {
            depth: 0,
            path: path.clone(),
            reference,
            default_open: false,
            name: expr.clone().into(),
            value: value.map(Into::into),
            type_: type_.map(Into::into),
            error,
            scope: false,
            note: false,
            watch: Some(ix),
        });
        if open {
            push_children(d, &mut rows, reference, &path, 1);
        }
    }
    rows
}

/// Draws `row`: chevron, name (`text` at the top, `property` below), the
/// type in muted, `=` and the value in syntax colours; a changed value
/// gets the `accent_alt` ring and "changed".
pub fn render_row(
    id: impl Into<ElementId>,
    row: Row,
    changed: bool,
    debugger: &Entity<Debugger>,
    cx: &App,
) -> AnyElement {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let d = debugger.read(cx);
    let expandable = row.reference > 0;
    let open = expandable && d.is_open(&row.path, row.default_open);
    let group = SharedString::from(format!("var-row-{}", row.path));
    // Fields read as properties: below a variable, or below a watch.
    let in_watch = row.path.starts_with('\u{1}');
    let name_color = if row.depth >= 2 || (row.depth == 1 && in_watch) {
        theme(cx).syntax("property").color.unwrap_or(c.text)
    } else {
        c.text
    };
    let (path, reference, default_open) = (row.path.clone(), row.reference, row.default_open);
    let toggle = debugger.downgrade();
    let remove = debugger.downgrade();
    div()
        .id(id)
        .group(group.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(ROW_H_DENSE)
        .mx(ROW_INSET)
        .pl(px(4. + row.depth as f32 * 14.))
        .pr(px(6.))
        .rounded(ROW_RADIUS)
        .whitespace_nowrap()
        .overflow_hidden()
        .when(!row.note, |el| el.hover(|s| s.bg(c.hover)))
        .when(expandable, |el| el.cursor_pointer())
        .child(div().w(px(14.)).flex_none().when(expandable, |el| {
            el.child(
                icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(13.))
                .color(c.text_muted),
            )
        }))
        .map(|el| {
            if row.scope || row.note {
                // Scope headers and notes are UI text, not code.
                el.child(
                    div()
                        .text_size(if row.note { scale.meta } else { scale.body })
                        .when(row.scope, |el| el.font_weight(WEIGHT_MEDIUM))
                        .text_color(if row.note { c.text_muted } else { c.text })
                        .child(row.name.clone()),
                )
            } else {
                // `name: Type = value`, the type and value without module
                // paths (the full type is in the tooltip).
                let type_ = row.type_.as_ref().map(|t| strip_paths(t));
                let value = row
                    .value
                    .clone()
                    .filter(|v| !v.is_empty())
                    .map(|v| SharedString::from(strip_paths(&v)));
                el.font_family(mono_font_family())
                    .text_size(scale.readout)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(180.))
                                    .overflow_hidden()
                                    .text_color(name_color)
                                    .child(row.name.clone()),
                            )
                            .when_some(type_, |el, t| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .max_w(px(140.))
                                        .overflow_hidden()
                                        .text_color(c.text_muted)
                                        .child(SharedString::from(format!(": {t}"))),
                                )
                            })
                            .when_some(value, |el, value| {
                                el.child(div().flex_none().text_color(c.text_muted).child("\u{a0}=\u{a0}"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_color(if row.error { c.error } else { c.text })
                                            .map(|el| {
                                                if row.error {
                                                    el.child(value)
                                                } else {
                                                    el.child(styled_value(value, cx))
                                                }
                                            }),
                                    )
                            }),
                    )
            }
        })
        .when(changed, |el| {
            el.child(
                div()
                    .ml_auto()
                    .pl(px(6.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .font_family(arcode_ui_kit::ui_font_family(cx))
                    .text_size(scale.micro)
                    .font_weight(WEIGHT_MEDIUM)
                    .text_color(c.accent_alt)
                    .child(unsaved_ring(None, cx))
                    .child("changed"),
            )
        })
        .when_some(row.watch, |el, w| {
            el.child(
                div()
                    .ml_auto()
                    .flex_none()
                    .invisible()
                    .group_hover(group, |s| s.visible())
                    .child(
                        icon_button(("rm-watch", w), IconName::X)
                            .size(px(18.))
                            .tooltip("Remove watch")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                remove.update(cx, |d, cx| d.remove_watch(w, cx)).ok();
                            }),
                    ),
            )
        })
        .when_some(row.type_.clone().filter(|_| !row.scope), |el, t| {
            el.tooltip(move |_, cx| Tooltip::text(t.clone(), cx))
        })
        .on_click(move |_, _, cx| {
            if reference > 0 {
                let path = path.clone();
                toggle
                    .update(cx, |d, cx| d.toggle_node(path, reference, default_open, cx))
                    .ok();
            }
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fit_on_one_line() {
        assert_eq!(one_line("Rect {\n  w: 3\n}"), "Rect {   w: 3 }");
        assert_eq!(
            first_line("\nerror: use of undeclared identifier 'total'\n 1 | total\n   | ^"),
            "error: use of undeclared identifier 'total'"
        );
        assert_eq!(first_line(""), "");
    }
}
