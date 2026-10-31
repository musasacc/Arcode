//! The Variables column: the scopes of the selected frame, expanded
//! lazily, with values that changed since the previous stop marked.
//! (Watch expressions live in the Run and Debug sidebar.)

use arcode_ui_kit::metrics::type_scale;
use arcode_ui_kit::theme;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::DebugPanel;
use super::changes::change_key;
use super::tree::{render_row, variable_rows};

impl DebugPanel {
    pub(super) fn render_variables(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let rows = variable_rows(d);
        let session = d.session();
        let stopped = session.is_some_and(|s| s.is_stopped());
        let generation = session.map_or(0, |s| s.generation);
        let frame = session
            .and_then(|s| s.selected_frame())
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let mut list: Vec<AnyElement> = Vec::new();
        if rows.is_empty() {
            list.push(
                div()
                    .px(px(12.))
                    .py(px(4.))
                    .text_size(type_scale(cx).meta)
                    .text_color(c.text_muted)
                    .child(if stopped { "Loading…" } else { "Not paused." })
                    .into_any_element(),
            );
        }
        let debugger = self.debugger.clone();
        for (ix, row) in rows.into_iter().enumerate() {
            let changed = match &row.value {
                Some(value) if !row.scope && !row.note && stopped => {
                    let key = change_key(&frame, &row.path);
                    self.changes.observe(generation, &key, value)
                }
                _ => false,
            };
            list.push(render_row(("var-row", ix), row, changed, &debugger, cx));
        }
        div()
            .id("debug-vars")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .pb(px(6.))
            .overflow_y_scroll()
            .track_scroll(&self.vars_scroll)
            .children(list)
    }
}
