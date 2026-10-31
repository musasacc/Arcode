//! The Debug tab of the bottom panel: a toolbar (continue, pause, steps,
//! restart, stop), the call stack, variables and watches, and the debug
//! console with a REPL.

mod console_view;
mod stack;
mod toolbar;
mod variables;

use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding,
    ScrollHandle, Subscription, UniformListScrollHandle, Window, actions, div, prelude::*, px,
    relative,
};

use super::Debugger;
use crate::text_field::TextField;

actions!(
    debug_panel,
    [SubmitRepl, ReplHistoryUp, ReplHistoryDown, AddWatch]
);

/// Key context of the console input.
const REPL_CONTEXT: &str = "DebugRepl";
/// Key context of the watch input.
const WATCH_CONTEXT: &str = "DebugWatchInput";

/// Enter evaluates (or adds a watch), Up/Down browse the REPL history.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", SubmitRepl, Some(REPL_CONTEXT)),
        KeyBinding::new("up", ReplHistoryUp, Some(REPL_CONTEXT)),
        KeyBinding::new("down", ReplHistoryDown, Some(REPL_CONTEXT)),
        KeyBinding::new("enter", AddWatch, Some(WATCH_CONTEXT)),
    ]);
}

/// The Debug tab content.
pub struct DebugPanel {
    debugger: Entity<Debugger>,
    repl: Entity<TextField>,
    watch_input: Entity<TextField>,
    console_scroll: UniformListScrollHandle,
    stack_scroll: ScrollHandle,
    vars_scroll: ScrollHandle,
    seen_console: u64,
    focus_handle: FocusHandle,
    _sub: Subscription,
}

impl Focusable for DebugPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DebugPanel {
    /// A panel showing `debugger`.
    pub fn new(debugger: Entity<Debugger>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&debugger, |panel, debugger, cx| {
            let version = debugger.read(cx).console_version();
            if version != panel.seen_console {
                panel.seen_console = version;
                let len = debugger.read(cx).console().len();
                if len > 0 {
                    panel
                        .console_scroll
                        .scroll_to_item(len - 1, gpui::ScrollStrategy::Top);
                }
            }
            cx.notify();
        });
        Self {
            debugger,
            repl: cx.new(|cx| TextField::new("Evaluate an expression", cx)),
            watch_input: cx.new(|cx| TextField::new("Add a watch expression", cx)),
            console_scroll: UniformListScrollHandle::new(),
            stack_scroll: ScrollHandle::new(),
            vars_scroll: ScrollHandle::new(),
            seen_console: 0,
            focus_handle: cx.focus_handle(),
            _sub: sub,
        }
    }

    /// Focuses the console input.
    pub fn focus_repl(&self, window: &mut Window, cx: &App) {
        window.focus(&self.repl.focus_handle(cx));
    }

    fn submit_repl(&mut self, _: &SubmitRepl, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.repl.read(cx).text().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.repl.update(cx, |f, cx| f.set_text("", cx));
        self.debugger.update(cx, |d, cx| d.evaluate_repl(text, cx));
    }

    fn history(&mut self, up: bool, cx: &mut Context<Self>) {
        let entry = self.debugger.update(
            cx,
            |d, _| {
                if up { d.history_up() } else { d.history_down() }
            },
        );
        if let Some(entry) = entry {
            self.repl.update(cx, |f, cx| f.set_text(&entry, cx));
        }
    }

    fn add_watch(&mut self, _: &AddWatch, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.watch_input.read(cx).text().to_string();
        self.watch_input.update(cx, |f, cx| f.set_text("", cx));
        self.debugger.update(cx, |d, cx| d.add_watch(text, cx));
    }

    fn column(title: &'static str, cx: &App) -> gpui::Div {
        let c = theme(cx).colors.clone();
        div().flex().flex_col().min_w_0().h_full().child(
            div()
                .flex_none()
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(c.text_muted)
                .child(title.to_uppercase()),
        )
    }
}

impl Render for DebugPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let has_session = d.session().is_some() || d.preparing().is_some();
        let body = if !has_session && d.console().is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_sm()
                .text_color(c.text_muted)
                .child(icon(IconName::Bug).size(px(24.)).color(c.text_disabled))
                .child("Start debugging with F5, or click a run button next to main or a test.")
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(
                    Self::column("Call Stack", cx)
                        .w(relative(0.24))
                        .flex_none()
                        .border_r_1()
                        .border_color(c.border)
                        .child(self.render_stack(cx)),
                )
                .child(
                    Self::column("Variables", cx)
                        .w(relative(0.34))
                        .flex_none()
                        .border_r_1()
                        .border_color(c.border)
                        .child(self.render_variables(cx)),
                )
                .child(
                    Self::column("Debug Console", cx)
                        .flex_1()
                        .child(self.render_console(cx)),
                )
                .into_any_element()
        };
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(c.surface)
            .child(self.render_toolbar(cx))
            .child(body)
    }
}
