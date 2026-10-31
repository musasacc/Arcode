//! The breakpoint dialog (right-click in the breakpoint lane or "Debug:
//! Edit Breakpoint"): condition, hit count and log message.

use arcode_ui_kit::Elevated;
use std::path::PathBuf;

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, theme};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    SharedString, Window, actions, div, prelude::*, px,
};

use super::breakpoints::Bp;
use crate::text_field::TextField;

actions!(breakpoint_dialog, [Save, Cancel, NextField, PreviousField]);

const CONTEXT: &str = "BreakpointDialog";

/// Enter saves, Escape cancels, Tab moves between the fields.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Save, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("tab", NextField, Some(CONTEXT)),
        KeyBinding::new("shift-tab", PreviousField, Some(CONTEXT)),
    ]);
}

/// What the dialog decided.
pub enum BpDialogEvent {
    /// Save these options (empty fields clear them).
    Save {
        /// The file.
        path: PathBuf,
        /// Zero-based line.
        line: usize,
        /// Condition expression.
        condition: String,
        /// Hit count.
        hit_condition: String,
        /// Log message.
        log_message: String,
    },
    /// Remove the breakpoint.
    Remove {
        /// The file.
        path: PathBuf,
        /// Zero-based line.
        line: usize,
    },
    /// Closed without changes.
    Dismissed,
}

/// The dialog view.
pub struct BreakpointDialog {
    path: PathBuf,
    line: usize,
    title: SharedString,
    exists: bool,
    fields: [Entity<TextField>; 3],
    focused: usize,
}

impl EventEmitter<BpDialogEvent> for BreakpointDialog {}

impl Focusable for BreakpointDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.fields[self.focused].focus_handle(cx)
    }
}

impl BreakpointDialog {
    /// A dialog for `path:line` showing `existing`'s options.
    pub fn new(
        path: PathBuf,
        line: usize,
        display: String,
        existing: Option<&Bp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = |placeholder: &'static str, text: Option<&String>, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut f = TextField::new(placeholder, cx);
                if let Some(text) = text {
                    f.set_text(text, cx);
                }
                f
            })
        };
        let fields = [
            field(
                "Break when this expression is true, e.g. i > 10",
                existing.and_then(|b| b.condition.as_ref()),
                cx,
            ),
            field(
                "Break after this many hits, e.g. 5",
                existing.and_then(|b| b.hit_condition.as_ref()),
                cx,
            ),
            field(
                "Log instead of breaking, e.g. i = {i}",
                existing.and_then(|b| b.log_message.as_ref()),
                cx,
            ),
        ];
        let focused = existing.map_or(0, |b| {
            if b.log_message.is_some() {
                2
            } else if b.hit_condition.is_some() && b.condition.is_none() {
                1
            } else {
                0
            }
        });
        Self {
            title: format!("Breakpoint at {display}:{}", line + 1).into(),
            path,
            line,
            exists: existing.is_some(),
            fields,
            focused,
        }
    }

    fn text(&self, ix: usize, cx: &App) -> String {
        self.fields[ix].read(cx).text().to_string()
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(BpDialogEvent::Save {
            path: self.path.clone(),
            line: self.line,
            condition: self.text(0, cx),
            hit_condition: self.text(1, cx),
            log_message: self.text(2, cx),
        });
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(BpDialogEvent::Dismissed);
    }

    fn move_focus(&mut self, delta: usize, window: &mut Window, cx: &mut Context<Self>) {
        let current = (0..self.fields.len())
            .find(|ix| self.fields[*ix].focus_handle(cx).is_focused(window))
            .unwrap_or(self.focused);
        self.focused = (current + delta) % self.fields.len();
        window.focus(&self.fields[self.focused].focus_handle(cx));
        cx.notify();
    }

    fn row(
        &self,
        ix: usize,
        label: &'static str,
        glyph: IconName,
        window: &Window,
        cx: &App,
    ) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let focused = self.fields[ix].focus_handle(cx).is_focused(window);
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .child(icon(glyph).size(px(14.)).color(c.text_muted))
            .child(
                div()
                    .w(px(96.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(c.text_muted)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .border_1()
                    .border_color(if focused { c.border_focused } else { c.border })
                    .bg(c.surface)
                    .overflow_hidden()
                    .child(self.fields[ix].clone()),
            )
    }
}

impl Render for BreakpointDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(|d, _: &NextField, w, cx| d.move_focus(1, w, cx)))
            .on_action(cx.listener(|d, _: &PreviousField, w, cx| d.move_focus(2, w, cx)))
            .w(px(520.))
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .child(
                div()
                    .pb_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(self.row(0, "Condition", IconName::CircleDot, window, cx))
            .child(self.row(1, "Hit count", IconName::History, window, cx))
            .child(self.row(2, "Log message", IconName::FileText, window, cx))
            .child(
                div()
                    .pt_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(c.text_muted)
                            .child("A log message makes it a log point."),
                    )
                    .when(self.exists, |el| {
                        el.child(
                            button("bp-remove", "Remove")
                                .style(ButtonStyle::Ghost)
                                .on_click(cx.listener(|d, _, _, cx| {
                                    cx.emit(BpDialogEvent::Remove {
                                        path: d.path.clone(),
                                        line: d.line,
                                    })
                                })),
                        )
                    })
                    .child(
                        button("bp-cancel", "Cancel")
                            .style(ButtonStyle::Secondary)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BpDialogEvent::Dismissed))),
                    )
                    .child(
                        button("bp-save", "Save")
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|d, _, w, cx| d.save(&Save, w, cx))),
                    ),
            )
    }
}
