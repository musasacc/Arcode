//! The Vim entry of the footer: a mode pill, then the command line being
//! typed, the pending keys of an unfinished command, the macro being
//! recorded and the last message.
//!
//! The pill follows the footer pills of docs/DESIGN.md 6.10: 20 tall,
//! radius 5, a solid `pill` fill, the mode in sentence case (Inter 12
//! Medium, `text`) after a 7 px status dot in the mode's colour.

use arcode_ui_kit::metrics::WEIGHT_MEDIUM;
use arcode_ui_kit::{status_dot, theme};
use gpui::{AnyElement, App, Hsla, IntoElement, ParentElement, Styled, div, prelude::*, px};

use super::VimStatus;
use arcode_vim::Mode;

/// The footer label of a mode, in sentence case (Vim's own labels are
/// upper case, which the footer never uses).
fn pill_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "Normal",
        Mode::Insert => "Insert",
        Mode::Replace => "Replace",
        Mode::Visual => "Visual",
        Mode::VisualLine => "Visual line",
        Mode::VisualBlock => "Visual block",
        Mode::OperatorPending => "Operator",
        Mode::CommandLine => "Command",
    }
}

/// The theme color of a mode's status dot.
fn mode_color(mode: Mode, cx: &App) -> Hsla {
    let c = &theme(cx).colors;
    match mode {
        Mode::Normal | Mode::OperatorPending | Mode::CommandLine => c.accent,
        Mode::Insert => c.success,
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => c.warning,
        Mode::Replace => c.error,
    }
}

/// True for Vim error messages (`E37: ...`).
fn is_error(message: &str) -> bool {
    let mut chars = message.chars();
    chars.next() == Some('E') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

/// Widest the entry gets, so it never pushes the status items away.
const MAX_WIDTH: f32 = 560.;
/// Longest command line shown; longer ones show their end (where the
/// caret is) after an ellipsis.
const MAX_TYPED_CHARS: usize = 56;
/// Longest message shown; longer ones end with an ellipsis.
const MAX_MESSAGE_CHARS: usize = 64;

/// The last `max` chars of `text`, after `…` if it was cut.
fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let rest: String = text.chars().skip(count - (max - 1)).collect();
    format!("…{rest}")
}

/// The first `max` chars of `text`, before `…` if it was cut.
fn head(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let start: String = text.chars().take(max - 1).collect();
    format!("{start}…")
}

/// The status bar element for `status`.
pub fn status_element(status: &VimStatus, cx: &App) -> AnyElement {
    let colors = &theme(cx).colors;
    let color = mode_color(status.kind, cx);
    let mono = arcode_ui_kit::settings(cx).editor.font_family.clone();
    let pill = div()
        .id("vim-mode")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .h(px(20.))
        .px(px(6.))
        .rounded(px(5.))
        .bg(colors.pill)
        .text_color(colors.text)
        .font_weight(WEIGHT_MEDIUM)
        .child(status_dot(color))
        .child(pill_label(status.kind));
    let typed = status
        .command_line
        .as_deref()
        .map(|line| tail(line, MAX_TYPED_CHARS))
        .or_else(|| (!status.pending.is_empty()).then(|| status.pending.clone()));
    // The entry shrinks before the other status items: long command lines
    // and messages are cut with an ellipsis instead of overlapping them.
    div()
        .id("vim-status")
        .flex()
        .items_center()
        .gap(px(6.))
        .h_full()
        .min_w_0()
        .max_w(px(MAX_WIDTH))
        .flex_shrink()
        .overflow_hidden()
        .child(pill)
        .when_some(status.recording, |el, reg| {
            el.child(
                div()
                    .flex_none()
                    .text_color(colors.error)
                    .child(format!("recording @{reg}")),
            )
        })
        .when_some(typed, |el, typed| {
            el.child(
                div()
                    .min_w_0()
                    .flex_shrink()
                    .truncate()
                    .font_family(mono)
                    .text_color(colors.text)
                    .child(typed),
            )
        })
        .when_some(status.message.clone(), |el, message| {
            let color = if is_error(&message) {
                colors.error
            } else {
                colors.text_muted
            };
            el.child(
                div()
                    .min_w_0()
                    .flex_shrink()
                    .truncate()
                    .text_color(color)
                    .child(head(&message, MAX_MESSAGE_CHARS)),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages() {
        assert!(is_error("E37: No write since last change"));
        assert!(!is_error("Echo"));
        assert!(!is_error("3 substitutions on 2 lines"));
    }

    #[test]
    fn pill_labels_are_sentence_case() {
        let modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Replace,
            Mode::Visual,
            Mode::VisualLine,
            Mode::VisualBlock,
            Mode::OperatorPending,
            Mode::CommandLine,
        ];
        for mode in modes {
            let label = pill_label(mode);
            let mut chars = label.chars();
            assert!(chars.next().is_some_and(|c| c.is_uppercase()), "{label}");
            assert!(chars.all(|c| !c.is_uppercase()), "{label}");
        }
        assert_eq!(pill_label(Mode::VisualLine), "Visual line");
    }

    #[test]
    fn long_command_lines_show_their_end() {
        assert_eq!(tail(":w", 5), ":w");
        assert_eq!(tail(":s/abcdef/x", 5), "…ef/x");
        assert_eq!(tail(":s/漢字漢字", 3), "…漢字");
        assert_eq!(head("E492: x", 9), "E492: x");
        assert_eq!(head("E492: xyz", 5), "E492…");
    }
}
