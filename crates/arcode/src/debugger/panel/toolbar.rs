//! The debug toolbar and status, drawn in the bottom panel's header while
//! the Debug tab is shown (docs/DESIGN.md 6.8): continue / pause, step
//! over, into, out, restart, stop; at the right the paused readout (ember
//! mark, "Paused", why, where and on which thread).

use arcode_dap::SessionState;
use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_H_PANEL, WEIGHT_SEMIBOLD, icon_size, type_scale};
use arcode_ui_kit::{
    ButtonStyle, IconName, button, ember, icon_button, mono_font_family, status_dot, theme,
    truncated,
};
use gpui::{Action, AnyElement, App, Hsla, IntoElement, SharedString, div, prelude::*, px};

use super::DebugPanel;
use crate::actions::shortcut_label;
use crate::debugger::actions::*;

/// One toolbar button, 28 wide and 26 tall with a 14 px icon. Disabled
/// buttons keep their place with a `text_disabled` icon (never opacity)
/// and do nothing.
#[allow(clippy::too_many_arguments)]
fn tool(
    id: &'static str,
    glyph: IconName,
    tip: &str,
    keys: &str,
    color: Option<Hsla>,
    enabled: bool,
    action: Box<dyn Action>,
    cx: &App,
) -> AnyElement {
    let c = &theme(cx).colors;
    let tooltip = format!("{tip} ({})", shortcut_label(keys));
    let mut b = icon_button(id, glyph)
        .size(CONTROL_H_PANEL)
        .width(CONTROL_H)
        .icon_size(icon_size::TOOLBAR)
        .tooltip(tooltip)
        .icon_color(if enabled {
            color.unwrap_or(c.text_muted)
        } else {
            c.text_disabled
        });
    if enabled {
        b = b.on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx));
    }
    b.into_any_element()
}

/// The status words at the right of the header for a session state.
pub fn status_words(state: &SessionState) -> (&'static str, Option<String>) {
    match state {
        SessionState::Initializing => ("Starting", None),
        SessionState::Running => ("Running", None),
        SessionState::Stopped { reason, .. } => ("Paused", Some(format!("on {reason}"))),
        SessionState::Terminated => ("Ended", None),
    }
}

impl DebugPanel {
    /// The toolbar and the status readout for the bottom panel header.
    pub fn render_header(&self, cx: &App) -> (AnyElement, AnyElement) {
        (self.render_toolbar(cx), self.render_status(cx))
    }

    fn render_toolbar(&self, cx: &App) -> AnyElement {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let session = d.session().filter(|s| !s.is_terminated());
        let stopped = session.is_some_and(|s| s.is_stopped());
        let running = session.is_some_and(|s| s.state == SessionState::Running);
        let live = session.is_some() || d.preparing().is_some();
        let bar = div().flex().flex_none().items_center().gap(px(2.));
        if !live {
            let start_label: SharedString = d
                .choices()
                .get(d.selected())
                .map_or_else(|| "Start debugging".into(), |c| c.name.clone().into());
            return bar
                .child(
                    button("dbg-start", start_label)
                        .icon(IconName::Play)
                        .icon_color(c.success)
                        .style(ButtonStyle::Ghost)
                        .compact()
                        .tooltip(format!("Start debugging ({})", shortcut_label("f5")))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(StartOrContinue), cx)
                        }),
                )
                .into_any_element();
        }
        let first = if stopped || !running {
            tool(
                "dbg-continue",
                IconName::CirclePlay,
                "Continue",
                "f5",
                Some(c.success),
                stopped,
                Box::new(Continue),
                cx,
            )
        } else {
            tool(
                "dbg-pause",
                IconName::Pause,
                "Pause",
                "f6",
                None,
                running,
                Box::new(Pause),
                cx,
            )
        };
        bar.child(first)
            .child(tool(
                "dbg-over",
                IconName::RedoDot,
                "Step over",
                "f10",
                None,
                stopped,
                Box::new(StepOver),
                cx,
            ))
            .child(tool(
                "dbg-into",
                IconName::ArrowDown,
                "Step into",
                "f11",
                None,
                stopped,
                Box::new(StepInto),
                cx,
            ))
            .child(tool(
                "dbg-out",
                IconName::ArrowUp,
                "Step out",
                "shift-f11",
                None,
                stopped,
                Box::new(StepOut),
                cx,
            ))
            .child(tool(
                "dbg-restart",
                IconName::RotateCcw,
                "Restart",
                "secondary-shift-f5",
                None,
                true,
                Box::new(RestartDebugging),
                cx,
            ))
            .child(
                icon_button("dbg-stop", IconName::Square)
                    .size(CONTROL_H_PANEL)
                    .width(CONTROL_H)
                    .icon_size(px(12.))
                    .icon_color(c.error)
                    .tooltip(format!("Stop ({})", shortcut_label("shift-f5")))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(StopDebugging), cx)),
            )
            .into_any_element()
    }

    /// `◆ Paused on breakpoint main.rs:11 · thread main` while paused; a
    /// dot and the state otherwise. The muted parts end in `…` when the
    /// header is short of room; the mark and the state word stay.
    fn render_status(&self, cx: &App) -> AnyElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let d = self.debugger.read(cx);
        let row = div()
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .whitespace_nowrap()
            .overflow_hidden()
            .text_size(scale.meta);
        let word = |text: SharedString| div().flex_none().font_weight(WEIGHT_SEMIBOLD).child(text);
        let muted = |text: String| {
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(c.text_muted)
                .child(truncated(text))
        };
        let mono = |text: String| {
            muted(text)
                .font_family(mono_font_family())
                .text_size(scale.readout)
        };
        if let Some(name) = d.preparing() {
            return row
                .child(status_dot(c.accent))
                .child(word("Building".into()))
                .child(muted(name.to_string()))
                .into_any_element();
        }
        let Some(s) = d.session() else {
            return row
                .child(muted("Not debugging".to_string()))
                .into_any_element();
        };
        let (state, why) = status_words(&s.state);
        if s.is_terminated() {
            let exit = s.exit_code.map(|code| format!("exit {code}"));
            return row
                .child(muted(format!("{} ended", s.name)))
                .children(exit.map(mono))
                .into_any_element();
        }
        if !s.is_stopped() {
            return row
                .child(status_dot(c.accent))
                .child(word(state.into()))
                .child(muted(s.name.clone()))
                .into_any_element();
        }
        let place = d.stopped_at().and_then(|(path, line)| {
            Some(format!(
                "{}:{}",
                path.file_name()?.to_string_lossy(),
                line + 1
            ))
        });
        let thread = s
            .thread
            .and_then(|id| s.threads.iter().find(|t| t.id == id))
            .map(|t| format!("thread {}", t.name));
        let location = [place, thread]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        row.child(ember())
            .child(word(state.into()))
            .children(why.map(muted))
            .when(!location.is_empty(), |el| el.child(mono(location)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_words_name_the_state() {
        assert_eq!(status_words(&SessionState::Running), ("Running", None));
        assert_eq!(status_words(&SessionState::Terminated), ("Ended", None));
        let paused = SessionState::Stopped {
            reason: "breakpoint".into(),
            thread_id: Some(1),
        };
        assert_eq!(
            status_words(&paused),
            ("Paused", Some("on breakpoint".to_string()))
        );
    }
}
