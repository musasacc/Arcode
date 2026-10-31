//! The debug toolbar: continue / pause, step over, into, out, restart,
//! stop, and the session's status.

use arcode_dap::SessionState;
use arcode_ui_kit::{ButtonStyle, IconName, button, icon_button, theme};
use gpui::{Action, AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

use super::DebugPanel;
use crate::actions::shortcut_label;
use crate::debugger::actions::*;

/// Shortcut label without the `secondary-` handling for plain keys.
fn key_label(keys: &str) -> String {
    shortcut_label(keys)
}

fn tool(
    id: &'static str,
    glyph: IconName,
    tip: &str,
    keys: &str,
    enabled: bool,
    action: Box<dyn Action>,
) -> AnyElement {
    let tooltip = format!("{tip} ({})", key_label(keys));
    div()
        .when(!enabled, |el| el.opacity(0.35))
        .child(
            icon_button(id, glyph)
                .size(px(24.))
                .tooltip(tooltip)
                .on_click(move |_, window, cx| {
                    if enabled {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }
                }),
        )
        .into_any_element()
}

impl DebugPanel {
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let session = d.session().filter(|s| !s.is_terminated());
        let stopped = session.is_some_and(|s| s.is_stopped());
        let running = session.is_some_and(|s| s.state == SessionState::Running);
        let live = session.is_some() || d.preparing().is_some();
        let status: SharedString = match (d.preparing(), session) {
            (Some(name), _) => format!("{name}: building…").into(),
            (None, Some(s)) => match &s.state {
                SessionState::Initializing => format!("{}: starting…", s.name).into(),
                SessionState::Running => format!("{}: running", s.name).into(),
                SessionState::Stopped { reason, .. } => {
                    format!("{}: paused ({reason})", s.name).into()
                }
                SessionState::Terminated => format!("{}: ended", s.name).into(),
            },
            (None, None) => match d.session() {
                Some(ended) => match ended.exit_code {
                    Some(code) => format!("{}: exited with code {code}", ended.name).into(),
                    None => format!("{}: ended", ended.name).into(),
                },
                None => "Not debugging".into(),
            },
        };
        let start_label: SharedString = d
            .choices()
            .get(d.selected())
            .map_or_else(|| "Start Debugging".into(), |c| c.name.clone().into());
        let mut bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .h(px(30.))
            .px_1()
            .border_b_1()
            .border_color(c.border);
        if live {
            bar = bar
                .child(if stopped || !running {
                    tool(
                        "dbg-continue",
                        IconName::Play,
                        "Continue",
                        "f5",
                        stopped,
                        Box::new(Continue),
                    )
                } else {
                    tool(
                        "dbg-pause",
                        IconName::Pause,
                        "Pause",
                        "f6",
                        running,
                        Box::new(Pause),
                    )
                })
                .child(tool(
                    "dbg-over",
                    IconName::RedoDot,
                    "Step Over",
                    "f10",
                    stopped,
                    Box::new(StepOver),
                ))
                .child(tool(
                    "dbg-into",
                    IconName::ArrowDownToDot,
                    "Step Into",
                    "f11",
                    stopped,
                    Box::new(StepInto),
                ))
                .child(tool(
                    "dbg-out",
                    IconName::ArrowUpFromDot,
                    "Step Out",
                    "shift-f11",
                    stopped,
                    Box::new(StepOut),
                ))
                .child(tool(
                    "dbg-restart",
                    IconName::RotateCcw,
                    "Restart",
                    "secondary-shift-f5",
                    true,
                    Box::new(RestartDebugging),
                ))
                .child(tool(
                    "dbg-stop",
                    IconName::Square,
                    "Stop",
                    "shift-f5",
                    true,
                    Box::new(StopDebugging),
                ));
        } else {
            bar = bar.child(
                button("dbg-start", start_label)
                    .icon(IconName::Play)
                    .style(ButtonStyle::Ghost)
                    .tooltip(format!("Start debugging ({})", key_label("f5")))
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(StartOrContinue), cx)
                    }),
            );
        }
        bar.child(
            div()
                .ml_2()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(if stopped { c.warning } else { c.text_muted })
                .child(status),
        )
        .child(
            icon_button("dbg-clear-console", IconName::Trash)
                .size(px(22.))
                .tooltip("Clear console")
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.debugger.update(cx, |d, cx| d.clear_console(cx));
                })),
        )
    }
}
