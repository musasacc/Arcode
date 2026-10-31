//! Variables (scopes of the selected frame, expanded lazily) and watch
//! expressions.

use arcode_dap::Variable;
use arcode_ui_kit::{IconName, icon, icon_button, theme};
use gpui::{AnyElement, App, Context, Hsla, IntoElement, SharedString, div, prelude::*, px};

use super::{DebugPanel, WATCH_CONTEXT};
use crate::debugger::inspect::{scope_open_by_default, scope_path, watch_path};
use crate::debugger::session::{NodePath, WatchValue, child_path};

/// Deepest nesting shown (adapters can report cyclic structures).
const MAX_DEPTH: usize = 16;

/// One row of the tree.
struct Row {
    depth: usize,
    path: NodePath,
    reference: i64,
    default_open: bool,
    name: SharedString,
    value: Option<SharedString>,
    type_: Option<SharedString>,
    error: bool,
    /// Watch index (shows a remove button).
    watch: Option<usize>,
}

impl DebugPanel {
    fn push_children(
        &self,
        rows: &mut Vec<Row>,
        reference: i64,
        path: &str,
        depth: usize,
        cx: &App,
    ) {
        let d = self.debugger.read(cx);
        let Some(s) = d.session() else {
            return;
        };
        let note = |text: &str| Row {
            depth,
            path: String::new(),
            reference: 0,
            default_open: false,
            name: text.to_string().into(),
            value: None,
            type_: None,
            error: false,
            watch: None,
        };
        let Some(children) = s.children.get(&reference) else {
            rows.push(note("Loading…"));
            return;
        };
        if children.is_empty() {
            rows.push(note("(empty)"));
        }
        for v in children.iter().take(1000) {
            let p = child_path(path, &v.name);
            let open = v.variables_reference > 0 && d.is_open(&p, false);
            rows.push(var_row(v, depth, p.clone()));
            if open && depth < MAX_DEPTH {
                self.push_children(rows, v.variables_reference, &p, depth + 1, cx);
            }
        }
    }

    fn tree_rows(&self, cx: &App) -> (Vec<Row>, Vec<Row>) {
        let d = self.debugger.read(cx);
        let mut vars = Vec::new();
        if let Some(s) = d.session() {
            for scope in &s.scopes {
                let path = scope_path(scope);
                let default_open = scope_open_by_default(scope);
                let open = d.is_open(&path, default_open);
                vars.push(Row {
                    depth: 0,
                    path: path.clone(),
                    reference: scope.variables_reference,
                    default_open,
                    name: scope.name.clone().into(),
                    value: None,
                    type_: None,
                    error: false,
                    watch: None,
                });
                if open {
                    self.push_children(&mut vars, scope.variables_reference, &path, 1, cx);
                }
            }
        }
        let mut watches = Vec::new();
        for (ix, (expr, value)) in d.watches().iter().enumerate() {
            let path = watch_path(ix);
            let (value, type_, reference, error) = match value {
                WatchValue::Pending => (None, None, 0, false),
                WatchValue::Value {
                    value,
                    type_,
                    reference,
                } => (Some(value.clone()), type_.clone(), *reference, false),
                WatchValue::Error(e) => (Some(e.clone()), None, 0, true),
            };
            let open = reference > 0 && d.is_open(&path, false);
            watches.push(Row {
                depth: 0,
                path: path.clone(),
                reference,
                default_open: false,
                name: expr.clone().into(),
                value: value.map(Into::into),
                type_: type_.map(Into::into),
                error,
                watch: Some(ix),
            });
            if open {
                self.push_children(&mut watches, reference, &path, 1, cx);
            }
        }
        (vars, watches)
    }

    fn render_row(&self, ix: usize, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let expandable = row.reference > 0;
        let open = expandable && d.is_open(&row.path, row.default_open);
        let value_color: Hsla = if row.error { c.error } else { c.text_muted };
        let (path, reference, default_open) = (row.path.clone(), row.reference, row.default_open);
        div()
            .id(("var-row", ix))
            .group("var-row")
            .flex()
            .items_center()
            .gap_1()
            .h(px(22.))
            .pl(px(6. + row.depth as f32 * 14.))
            .pr_1()
            .when(expandable, |el| el.cursor_pointer())
            .hover(|s| s.bg(c.hover))
            .child(div().w(px(12.)).flex_none().when(expandable, |el| {
                el.child(
                    icon(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(12.))
                    .color(c.text_muted),
                )
            }))
            .child(
                div()
                    .flex_none()
                    .max_w(px(160.))
                    .truncate()
                    .when(row.depth == 0 && row.watch.is_none(), |el| {
                        el.font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .child(row.name.clone()),
            )
            .when_some(row.value.clone().filter(|v| !v.is_empty()), |el, value| {
                el.child(div().flex_none().text_color(c.text_muted).child("="))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(value_color)
                            .child(value),
                    )
            })
            .when_some(row.type_.clone(), |el, t| {
                el.tooltip(move |_, cx| arcode_ui_kit::Tooltip::text(t.clone(), cx))
            })
            .when_some(row.watch, |el, w| {
                el.child(
                    div()
                        .ml_auto()
                        .opacity(0.)
                        .group_hover("var-row", |s| s.opacity(1.))
                        .child(
                            icon_button(("rm-watch", w), IconName::X)
                                .size(px(18.))
                                .tooltip("Remove watch")
                                .on_click(cx.listener(move |panel, _, _, cx| {
                                    cx.stop_propagation();
                                    panel.debugger.update(cx, |d, cx| d.remove_watch(w, cx));
                                })),
                        ),
                )
            })
            .on_click(cx.listener(move |panel, _, _, cx| {
                if reference > 0 {
                    let path = path.clone();
                    panel
                        .debugger
                        .update(cx, |d, cx| d.toggle_node(path, reference, default_open, cx));
                }
            }))
            .into_any_element()
    }

    pub(super) fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let (vars, watches) = self.tree_rows(cx);
        let stopped = self
            .debugger
            .read(cx)
            .session()
            .is_some_and(|s| s.is_stopped());
        let mut list: Vec<AnyElement> = Vec::new();
        if vars.is_empty() {
            list.push(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(if stopped { "Loading…" } else { "Not paused." })
                    .into_any_element(),
            );
        }
        let n = vars.len();
        for (ix, row) in vars.into_iter().enumerate() {
            list.push(self.render_row(ix, row, cx));
        }
        list.push(
            div()
                .mt_2()
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(c.text_muted)
                .child("WATCH")
                .into_any_element(),
        );
        for (ix, row) in watches.into_iter().enumerate() {
            list.push(self.render_row(n + ix, row, cx));
        }
        list.push(
            div()
                .key_context(WATCH_CONTEXT)
                .on_action(cx.listener(Self::add_watch))
                .mx_2()
                .my_1()
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(c.border)
                .bg(c.background)
                .child(icon(IconName::Plus).size(px(12.)).color(c.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.watch_input.clone()),
                )
                .into_any_element(),
        );
        div()
            .id("debug-vars")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.vars_scroll)
            .text_sm()
            .children(list)
    }
}

fn var_row(v: &Variable, depth: usize, path: NodePath) -> Row {
    Row {
        depth,
        path,
        reference: v.variables_reference,
        default_open: false,
        name: v.name.clone().into(),
        value: Some(v.value.replace('\n', " ").into()),
        type_: v.type_.clone().map(Into::into),
        error: false,
        watch: None,
    }
}
