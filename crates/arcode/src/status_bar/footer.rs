//! The footer (docs/DESIGN.md 6.10): the status bar redrawn on the canvas
//! with no fill and no border, from the rail's right edge to 6 px before
//! the window edge. Items are 20 px pills; every number is a mono readout.

use arcode_ui_kit::metrics::{FOOTER_H, GAP, RAIL_W, WEIGHT_MEDIUM};
use arcode_ui_kit::{IconName, Tooltip, icon, readout, settings, theme, truncated, type_scale};
use gpui::{
    AnyView, App, Context, ElementId, Hsla, IntoElement, SharedString, Stateful, Window, div,
    prelude::*, px,
};

use super::{Startup, StatusBar, conflict_label, cursor_label};
use crate::terminal_panel::RunStatus;

/// Footer icon size.
const ICON: f32 = 13.;

/// A footer item: 20 tall, radius 5, padding 0 6, gap 5. `clickable`
/// items get a solid `hover` fill.
pub fn item(id: impl Into<ElementId>, clickable: bool, cx: &App) -> Stateful<gpui::Div> {
    let hover = theme(cx).colors.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .h(px(20.))
        .px(px(6.))
        .rounded(px(5.))
        .whitespace_nowrap()
        .when(clickable, |el| {
            el.cursor_pointer().hover(move |s| s.bg(hover))
        })
}

/// The footer row: no fill, starts at the rail's right edge.
fn row(cx: &App) -> gpui::Div {
    let c = &theme(cx).colors;
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .h(FOOTER_H)
        .pl(RAIL_W)
        .pr(GAP)
        .text_size(type_scale(cx).meta)
        .font_weight(WEIGHT_MEDIUM)
        .text_color(c.text_muted)
}

/// The memory readout: `monitor` and mono `164 MB`.
fn memory_item(mib: u64, cx: &App) -> impl IntoElement + use<> {
    let c = &theme(cx).colors;
    item("status-memory", false, cx)
        .child(icon(IconName::Monitor).size(px(ICON)).color(c.text_muted))
        .child(readout(format!("{mib} MB")))
        .tooltip(|_, cx| -> AnyView { Tooltip::text("Memory in use", cx) })
}

/// The footer of the welcome screen: only the memory figure (shown while
/// perf stats are on, like everywhere else).
pub fn welcome_footer(memory: Option<u64>, cx: &App) -> impl IntoElement + use<> {
    row(cx)
        .child(div())
        .when_some(memory, |el, mib| el.child(memory_item(mib, cx)))
}

/// Splits a run readout into its status part and the rest:
/// `exit 101 · 0.92 s` gives `("exit 101", Some("· 0.92 s"))`.
pub fn split_readout(text: &str) -> (&str, Option<&str>) {
    match text.find(" · ") {
        Some(ix) => (&text[..ix], Some(&text[ix + 1..])),
        None => (text, None),
    }
}

/// The color of the status part of a run readout.
fn run_color(status: RunStatus, cx: &App) -> Hsla {
    let c = &theme(cx).colors;
    match status {
        RunStatus::Failed => c.error,
        RunStatus::Succeeded => c.success,
        RunStatus::Running | RunStatus::Ended => c.text_muted,
    }
}

impl StatusBar {
    /// The last run (`history`, the command, `exit 101 · 0.92 s`), shown
    /// only while the bottom panel, which has its own readout, is hidden.
    fn run_item(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let (terminals, panel_hidden) = self.run_source.as_ref()?;
        if !panel_hidden.get() {
            return None;
        }
        let run = terminals.read(cx).last_run()?;
        let c = theme(cx).colors.clone();
        let text = run.readout();
        let (head, tail) = split_readout(&text);
        let head_color = run_color(run.status(), cx);
        let command = run.command.clone();
        Some(
            item("status-run", true, cx)
                .child(icon(IconName::History).size(px(ICON)).color(c.text_muted))
                .child(
                    div()
                        .max_w(px(200.))
                        .min_w_0()
                        .overflow_hidden()
                        .text_color(c.text)
                        .child(truncated(command.clone())),
                )
                .child(readout(head.to_string()).color(head_color))
                .when_some(tail.map(str::to_string), |el, tail| el.child(readout(tail)))
                .tooltip(move |_, cx| -> AnyView {
                    Tooltip::text(format!("{command}: show the terminal"), cx)
                })
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::ToggleBottomPanel), cx)
                }),
        )
    }
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let info = &self.info;
        let perf = settings(cx).ui.show_perf_stats;
        let ready = cx.try_global::<Startup>().and_then(|s| s.ready);
        let icon_size = px(ICON);
        let count_color = |n: usize, color: Hsla| if n > 0 { color } else { c.text_muted };
        let left = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .when_some(self.vim.as_ref(), |el, vim| {
                el.child(arcode_editor::vim::status_element(vim, cx))
            })
            .when_some(info.branch.clone(), |el, branch| {
                el.child(
                    item("status-branch", true, cx)
                        .min_w_0()
                        .flex_shrink()
                        .child(
                            icon(IconName::GitBranch)
                                .size(icon_size)
                                .color(c.text_muted),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(c.text)
                                .child(truncated(branch)),
                        )
                        .tooltip(|_, cx| -> AnyView { Tooltip::text("Switch branch", cx) })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::SwitchBranch), cx)
                        }),
                )
            })
            .child(
                item("status-errors", false, cx)
                    .child(
                        icon(IconName::CircleX)
                            .size(icon_size)
                            .color(count_color(info.errors, c.error)),
                    )
                    .child(
                        readout(info.errors.to_string()).color(count_color(info.errors, c.error)),
                    )
                    .child(div().w(px(3.)))
                    .child(
                        icon(IconName::TriangleAlert)
                            .size(icon_size)
                            .color(count_color(info.warnings, c.warning)),
                    )
                    .child(
                        readout(info.warnings.to_string())
                            .color(count_color(info.warnings, c.warning)),
                    ),
            )
            .when_some(info.conflicts, |el, (current, total)| {
                el.child(
                    item("status-conflicts", true, cx)
                        .text_color(c.warning)
                        .child(icon(IconName::GitMerge).size(icon_size).color(c.warning))
                        .child(conflict_label(current, total))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::NextConflict), cx)
                        }),
                )
            })
            .children(self.run_item(cx))
            .when_some(self.plugin_view.clone(), |el, view| el.child(view));
        let right = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h_full()
            .flex_none()
            .when_some(info.cursor, |el, (line, col)| {
                el.child(item("status-cursor", false, cx).child(readout(cursor_label(line, col))))
            })
            .children(crate::languages::status_items::render(
                self.doc.as_ref(),
                info.language.clone(),
                cx,
            ))
            .when_some(self.lsp_view.clone(), |el, view| el.child(view))
            .when(self.lsp_view.is_none(), |el| {
                el.when_some(info.lsp_status.clone(), |el, status| {
                    el.child(item("status-lsp", false, cx).child(status))
                })
            })
            .when(perf, |el| {
                el.when_some(ready, |el, ready| {
                    el.child(
                        item("status-ready", false, cx).child(readout(SharedString::from(
                            format!("ready in {} ms", ready.as_millis()),
                        ))),
                    )
                })
                .when_some(self.memory_mib, |el, mib| el.child(memory_item(mib, cx)))
            });
        row(cx).child(left).child(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readouts_split_after_the_status() {
        assert_eq!(
            split_readout("exit 101 · 0.92 s"),
            ("exit 101", Some("· 0.92 s"))
        );
        assert_eq!(split_readout("running · 4 s"), ("running", Some("· 4 s")));
        assert_eq!(split_readout("started"), ("started", None));
    }
}
