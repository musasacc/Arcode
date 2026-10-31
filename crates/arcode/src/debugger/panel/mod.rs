//! The Debug tab of the bottom panel (docs/DESIGN.md 6.8): three columns
//! (call stack, variables, debug console with a REPL) divided by
//! hairlines. The toolbar (continue, pause, steps, restart, stop) and the
//! paused readout are drawn in the bottom panel's header.

pub(crate) mod changes;
mod console_view;
mod stack;
mod toolbar;
pub(crate) mod tree;
mod value_style;
mod variables;

use arcode_ui_kit::metrics::type_scale;
use arcode_ui_kit::{IconName, icon, icon_button, section_header, spacer, theme, truncated};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding,
    ScrollHandle, SharedString, Subscription, UniformListScrollHandle, Window, actions, div,
    prelude::*, px,
};

use super::Debugger;
use crate::text_field::TextField;

actions!(
    debug_panel,
    [SubmitRepl, ReplHistoryUp, ReplHistoryDown, AddWatch]
);

/// Key context of the console input.
const REPL_CONTEXT: &str = "DebugRepl";
/// Key context of the watch input (in the Run and Debug sidebar).
pub(crate) const WATCH_CONTEXT: &str = "DebugWatchInput";
/// Width of the call stack and console columns.
const SIDE_COLUMN_W: f32 = 330.;

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
    console_scroll: UniformListScrollHandle,
    stack_scroll: ScrollHandle,
    vars_scroll: ScrollHandle,
    seen_console: u64,
    /// Values that changed since the previous stop.
    changes: changes::ValueChanges,
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
            console_scroll: UniformListScrollHandle::new(),
            stack_scroll: ScrollHandle::new(),
            vars_scroll: ScrollHandle::new(),
            seen_console: 0,
            changes: Default::default(),
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

    /// A column: a section header, then `body`.
    fn column(header: gpui::Div, body: impl IntoElement) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .h_full()
            .child(header)
            .child(body)
    }
}

impl Render for DebugPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let d = self.debugger.read(cx);
        let has_session = d.session().is_some() || d.preparing().is_some();
        if !has_session && d.console().is_empty() {
            return div()
                .track_focus(&self.focus_handle)
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_size(type_scale(cx).body)
                .text_color(c.text_muted)
                .child(icon(IconName::Bug).size(px(24.)).color(c.text_disabled))
                .child("Start debugging with F5, or click a run button next to main or a test.");
        }
        let thread: Option<SharedString> = d.session().and_then(|s| {
            let id = s.thread?;
            let t = s.threads.iter().find(|t| t.id == id)?;
            Some(format!("thread {}", t.name).into())
        });
        let adapter: SharedString = d
            .session()
            .map(|s| s.adapter.clone())
            .unwrap_or_default()
            .into();
        let muted = |text: SharedString| {
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(c.text_muted)
                .font_weight(gpui::FontWeight::NORMAL)
                .child(truncated(text))
        };
        let repl_focused = self.repl.focus_handle(cx).is_focused(window);
        let stack = Self::column(
            section_header("Call stack", cx).children(thread.map(muted)),
            self.render_stack(cx),
        )
        .flex_basis(px(SIDE_COLUMN_W))
        .flex_shrink()
        .min_w(px(140.));
        let variables = Self::column(section_header("Variables", cx), self.render_variables(cx))
            .flex_1()
            .min_w(px(160.));
        let console = Self::column(
            section_header("Debug console", cx)
                .child(spacer())
                .child(muted(adapter))
                .child(
                    icon_button("dbg-clear-console", IconName::Trash)
                        .size(px(22.))
                        .tooltip("Clear console")
                        .on_click(cx.listener(|panel, _, _, cx| {
                            panel.debugger.update(cx, |d, cx| d.clear_console(cx));
                        })),
                ),
            self.render_console(repl_focused, cx),
        )
        .flex_basis(px(SIDE_COLUMN_W))
        .flex_shrink()
        .min_w(px(160.));
        let rule = || div().flex_none().w(px(1.)).h_full().bg(c.border);
        // No fill: the bottom panel's rounded fill shows through.
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .child(stack)
            .child(rule())
            .child(variables)
            .child(rule())
            .child(console)
    }
}
