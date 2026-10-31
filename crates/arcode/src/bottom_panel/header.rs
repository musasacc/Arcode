//! The bottom panel's 38 px header (docs/DESIGN.md 6.8): the tab pills,
//! then the tab's own controls (the debug toolbar and paused readout, or
//! the run readout and the terminal sessions), then the close `x`.
//!
//! When the header is short of room it gives way in this order: the run
//! readout or debug status ends in `…` first, then the tabs drop their
//! labels (icon and badge stay, the label becomes the tooltip), then the
//! tabs and terminal sessions scroll. The close `x` and the `+` for a new
//! terminal always stay in view.

use arcode_ui_kit::metrics::{CONTROL_H_PANEL, TAB_STRIP_H};
use arcode_ui_kit::{IconName, icon_button, pill_tab, spacer, theme};
use gpui::{AnyElement, Context, Div, Hsla, Pixels, canvas, div, prelude::*, px};

use super::{BottomPanel, BottomTab};
use crate::actions::{ToggleBottomPanel, shortcut_label};
use crate::terminal_panel;

/// Below this header width the tabs show only their icons and badges.
pub const LABELS_MIN_WIDTH: Pixels = px(760.);

/// Whether a header `width` wide shows the tab labels.
pub fn shows_tab_labels(width: Pixels) -> bool {
    width >= LABELS_MIN_WIDTH
}

/// How much faster than the tabs and sessions the readouts give way (a
/// flex-shrink factor: they are cut long before anything else shrinks).
const READOUT_SHRINK: f32 = 100.;

impl BottomPanel {
    /// The header row.
    pub(super) fn render_header(&self, focused: bool, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let debug_header = match (&self.debug, self.active) {
            (Some(debug), BottomTab::Debug) => Some(debug.read(cx).render_header(cx)),
            _ => None,
        };
        let readout = (self.active != BottomTab::Debug)
            .then(|| terminal_panel::render_run_readout(&self.terminals, cx))
            .flatten();
        // Measures the header so the tabs can drop their labels; a new
        // frame is asked for only when the answer flips.
        let panel = cx.entity().downgrade();
        let labels = self.tab_labels;
        let measure = canvas(
            move |bounds, _, cx| {
                let now = shows_tab_labels(bounds.size.width);
                if now != labels {
                    panel
                        .update(cx, |panel, cx| {
                            panel.tab_labels = now;
                            cx.notify();
                        })
                        .ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let header = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(TAB_STRIP_H)
            .px(px(6.))
            .child(measure)
            .child(self.render_tabs(focused, cx));
        let terminal = self.active == BottomTab::Terminal;
        let header = match debug_header {
            // Debug: the toolbar follows the tabs, the paused readout sits
            // at the right.
            Some((toolbar, status)) => header
                .child(header_rule(c.border))
                .child(toolbar)
                .child(spacer())
                .child(gives_way_first(status))
                .child(header_rule(c.border)),
            None => header
                .child(spacer())
                .when_some(readout, |el, readout| {
                    el.child(gives_way_first(readout))
                        .when(terminal, |el| el.child(header_rule(c.border)))
                })
                .when(terminal, |el| {
                    el.child(terminal_panel::render_strip(&self.terminals, cx))
                }),
        };
        header.child(
            icon_button("close-panel", IconName::X)
                .size(CONTROL_H_PANEL)
                .tooltip(format!("Close panel ({})", shortcut_label("secondary-j")))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleBottomPanel), cx)),
        )
    }

    /// The tab pills: Terminal, Problems, Todo and Debug with their count
    /// badges; the active one carries the ember mark while the panel has
    /// focus. Without labels in a narrow header; they scroll when even
    /// that does not fit.
    fn render_tabs(&self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let problems = self.problems.read(cx);
        let (problem_count, has_errors) = (problems.count(), problems.has_errors());
        let todo_count = self.todo.as_ref().map_or(0, |(s, _, _)| s.read(cx).count());
        let icon_only = !self.tab_labels;
        div()
            .id("bottom-tabs")
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .gap(px(3.))
            .overflow_x_scroll()
            .children(BottomTab::ALL.into_iter().map(|tab| {
                let active = tab == self.active;
                let count = match tab {
                    BottomTab::Problems => problem_count,
                    BottomTab::Todo => todo_count,
                    _ => 0,
                };
                let mut pill = pill_tab(tab.label(), tab.label())
                    .panel()
                    .icon(tab.icon(), None)
                    .icon_only(icon_only)
                    .active(active)
                    .focused(focused)
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.set_active_tab(tab, cx);
                        panel.focus(window, cx);
                    }));
                if count > 0 {
                    let color = (tab == BottomTab::Problems && has_errors).then_some(c.error);
                    pill = pill.badge(count.to_string(), color);
                }
                pill
            }))
    }
}

/// Wraps a readout so it gives up its room before anything else in the
/// header (it ends in `…`, down to nothing).
fn gives_way_first(readout: AnyElement) -> Div {
    let mut el = div().flex().min_w_0().overflow_hidden().child(readout);
    el.style().flex_shrink = Some(READOUT_SHRINK);
    el
}

/// A 16 px tall vertical hairline between groups of header controls.
fn header_rule(color: Hsla) -> Div {
    div().flex_none().w(px(1.)).h(px(16.)).mx(px(2.)).bg(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_headers_drop_the_tab_labels() {
        assert!(shows_tab_labels(px(760.)));
        assert!(shows_tab_labels(px(1400.)));
        assert!(!shows_tab_labels(px(759.)));
        assert!(!shows_tab_labels(px(0.)));
    }
}
