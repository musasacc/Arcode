//! Floating layers above the workspace: the project menu, the file finder
//! and other pickers, the path prompt and toast messages. Pickers sit 4 px
//! under the title bar with no dim behind them (docs/DESIGN.md 5.2, 6.11);
//! toasts sit at the bottom right, 8 px above the footer (6.12). Each fades
//! in when animations are enabled.

use arcode_ui_kit::metrics::{FOOTER_H, POPOVER_RADIUS, RAIL_W};
use arcode_ui_kit::{
    Elevated, IconName, animation_duration, icon, icon_button, theme, type_scale, ui_font_family,
};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Div, ElementId, MouseButton, Stateful,
    Window, div, ease_out_quint, prelude::*, px,
};

use super::Workspace;
use crate::picker_frame::TOP_GAP;
use crate::title_bar::TITLE_BAR_HEIGHT;

/// Wraps `el` in a short fade-in, or returns it unchanged when animations
/// are off.
pub fn fade_in(id: impl Into<ElementId>, el: Div, cx: &App) -> AnyElement {
    match animation_duration(cx) {
        Some(duration) => el
            .with_animation(
                id,
                Animation::new(duration).with_easing(ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
        None => el.into_any_element(),
    }
}

impl Workspace {
    fn backdrop(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
        on_close: fn(&mut Self, &mut Window, &mut Context<Self>),
    ) -> Stateful<Div> {
        div().id(id).absolute().inset_0().occlude().on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _, window, cx| on_close(ws, window, cx)),
        )
    }

    /// Adds the visible overlays as the last children of `root`.
    pub(super) fn render_overlays(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let mut root = root;
        if self.project_menu_open {
            let menu = self.render_project_menu(cx);
            root = root.child(
                self.backdrop("menu-backdrop", cx, |ws, _, cx| {
                    ws.project_menu_open = false;
                    cx.notify();
                })
                .child(
                    div()
                        .absolute()
                        .top(px(TITLE_BAR_HEIGHT - 2.))
                        .left(RAIL_W)
                        .child(fade_in("project-menu-anim", menu, cx)),
                ),
            );
        }
        if let Some(finder) = &self.finder {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(finder.clone());
            root = root.child(
                self.backdrop("finder-backdrop", cx, |ws, window, cx| {
                    ws.close_finder(window, cx)
                })
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + TOP_GAP))
                .child(fade_in("finder-anim", panel, cx)),
            );
        }
        if let Some(picker) = &self.branch_picker {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(picker.clone());
            root = root.child(
                self.backdrop("branch-backdrop", cx, |ws, window, cx| {
                    ws.close_branch_picker(window, cx)
                })
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + TOP_GAP))
                .child(fade_in("branch-anim", panel, cx)),
            );
        }
        if let Some((picker, _)) = &self.langs.picker {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(picker.clone());
            root = root.child(
                self.backdrop("list-picker-backdrop", cx, |ws, window, cx| {
                    ws.close_list_picker(window, cx)
                })
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + TOP_GAP))
                .child(fade_in("list-picker-anim", panel, cx)),
            );
        }
        if let Some(prompt) = &self.path_prompt {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(prompt.clone());
            root = root.child(
                self.backdrop("path-backdrop", cx, |ws, window, cx| {
                    ws.path_prompt = None;
                    ws.path_prompt_then = None;
                    ws.active_pane().read(cx).focus(window, cx);
                    cx.notify();
                })
                .flex()
                .justify_center()
                .pt(px(TITLE_BAR_HEIGHT + TOP_GAP))
                .child(fade_in("path-anim", panel, cx)),
            );
        }
        if let Some(toast) = &self.toast {
            let (glyph, color) = match self.toast_level {
                ToastLevel::Error => (IconName::CircleX, c.error),
                ToastLevel::Warning => (IconName::TriangleAlert, c.warning),
                ToastLevel::Info => (IconName::Info, c.info),
            };
            let toast_el = div()
                .max_w(px(480.))
                .min_h(px(36.))
                .pl(px(12.))
                .pr(px(4.))
                .py(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(POPOVER_RADIUS)
                .border_1()
                .border_color(c.border_strong)
                .bg(c.panel_raised())
                .elevated(cx)
                .font_family(ui_font_family(cx))
                .text_size(type_scale(cx).body)
                .text_color(c.text)
                .child(icon(glyph).size(px(14.)).color(color))
                .child(toast_text(toast))
                .child(
                    icon_button("toast-close", IconName::X)
                        .size(px(24.))
                        .tooltip("Dismiss")
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.toast = None;
                            ws.toast_task = None;
                            cx.notify();
                        })),
                );
            root = root.child(
                div()
                    .absolute()
                    .right(px(14.))
                    .bottom(FOOTER_H + px(8.))
                    .child(fade_in(("toast-anim", self.toast_seq), toast_el, cx)),
            );
        }
        root
    }
}

/// Longest chunk of a toast kept on one line; longer ones (a long path)
/// may wrap inside themselves rather than be clipped.
const TOAST_CHUNK_NOWRAP: usize = 48;

/// The words of a toast message, a `code span` (with the punctuation
/// around it) counting as one word, so a wrapped toast never breaks inside
/// `cargo run` or starts its second line with `)`.
pub(super) fn toast_chunks(message: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let (mut start, mut in_code) = (None, false);
    for (ix, ch) in message.char_indices() {
        if ch == '`' {
            in_code = !in_code;
        }
        if ch.is_whitespace() && !in_code {
            if let Some(s) = start.take() {
                chunks.push(&message[s..ix]);
            }
        } else if start.is_none() {
            start = Some(ix);
        }
    }
    if let Some(s) = start {
        chunks.push(&message[s..]);
    }
    chunks
}

/// A toast's message: its chunks (see [`toast_chunks`]) wrapped as words.
fn toast_text(message: &str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_wrap()
        .gap_x(px(4.))
        .children(toast_chunks(message).into_iter().map(|chunk| {
            div()
                .min_w_0()
                .max_w_full()
                .when(chunk.chars().count() <= TOAST_CHUNK_NOWRAP, |el| {
                    el.whitespace_nowrap()
                })
                .child(chunk.to_string())
        }))
}

/// The status icon of a toast (docs/DESIGN.md 6.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    /// `circle-x` in `error`: something the user asked for did not happen.
    Error,
    /// `triangle-alert` in `warning`.
    Warning,
    /// `info` in `info`.
    Info,
}

/// The level of a toast shown with [`Workspace::show_toast`], from its
/// wording ("Cannot start a terminal", "save failed: …", a plugin's
/// "plugin `x` is disabled"). Callers that know the level use
/// [`Workspace::show_toast_as`].
pub(super) fn toast_level(message: &str) -> ToastLevel {
    let lower = message.to_lowercase();
    let error = [
        "cannot ",
        "can't ",
        "could not",
        "couldn't",
        "failed",
        "error",
        "refused",
        // The plugin manager's errors (`ManagerError`).
        "no plugin `",
        "no contribution `",
        "` is disabled",
    ]
    .iter()
    .any(|w| lower.contains(w));
    // Something missing that the user can install or set up.
    let warning = ["not installed", "not on path", "not found", "warning"]
        .iter()
        .any(|w| lower.contains(w));
    if error {
        ToastLevel::Error
    } else if warning {
        ToastLevel::Warning
    } else {
        ToastLevel::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_level_follows_the_wording() {
        assert_eq!(toast_level("Cannot start a terminal"), ToastLevel::Error);
        assert_eq!(toast_level("git status failed: x"), ToastLevel::Error);
        assert_eq!(toast_level("Copied path"), ToastLevel::Info);
        assert_eq!(toast_level("Saved 3 files"), ToastLevel::Info);
        assert_eq!(
            toast_level(
                "Language server taplo is not installed (command: `taplo lsp stdio`). \
                 Install it, then restart it from the status bar."
            ),
            ToastLevel::Warning
        );
        assert_eq!(
            toast_level("pnpm is not installed (or not on PATH)."),
            ToastLevel::Warning
        );
    }

    #[test]
    fn code_spans_stay_in_one_chunk() {
        assert_eq!(
            toast_chunks("server taplo (command: `taplo lsp stdio`). Install it"),
            vec!["server", "taplo", "(command:", "`taplo lsp stdio`).", "Install", "it"]
        );
        assert_eq!(toast_chunks("  a   b "), vec!["a", "b"]);
        assert!(toast_chunks("").is_empty());
        // An unclosed span runs to the end.
        assert_eq!(toast_chunks("run `a b"), vec!["run", "`a b"]);
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    /// Errors that reach `show_toast` today without one of the guessed
    /// words: a plugin command or panel that fails (`plugins_cmd.rs`,
    /// `plugins_open.rs` format `"{name}: {e}"` with a `ManagerError`).
    /// They get the blue `info` icon, so the status icon (DESIGN 2.4 rule
    /// 1, 6.12) says the opposite of what happened.
    #[test]
    fn plugin_failures_read_as_errors() {
        assert_eq!(
            toast_level("HTTP Client: plugin `http` is disabled"),
            ToastLevel::Error
        );
        assert_eq!(
            toast_level("Send Request: no contribution `http.send`"),
            ToastLevel::Error
        );
    }
}
