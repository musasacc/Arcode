//! `Render` for [`Editor`]: key context, action handlers, the element and
//! the diagnostic hover popover.

use arcode_ui_kit::{icon, settings, theme};
use gpui::{
    AnyElement, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    anchored, deferred, div, prelude::*, px,
};

use crate::Editor;
use crate::actions::KEY_CONTEXT;
use crate::element::EditorElement;
use crate::mouse::HoverState;

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_blink(window, cx);
        if !self.focus_handle.is_focused(window) {
            self.completion_blurred(cx);
        }
        let popover = self.hover.clone().map(|h| hover_popover(&h, cx));
        div()
            .id("editor")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_hidden()
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::move_word_left))
            .on_action(cx.listener(Self::move_word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::move_home))
            .on_action(cx.listener(Self::move_end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_action(cx.listener(Self::move_doc_start))
            .on_action(cx.listener(Self::move_doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::undo_action))
            .on_action(cx.listener(Self::redo_action))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::select_next_occurrence))
            .on_action(cx.listener(Self::add_cursor_above))
            .on_action(cx.listener(Self::add_cursor_below))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::toggle_line_comment))
            .on_action(cx.listener(Self::move_line_up))
            .on_action(cx.listener(Self::move_line_down))
            .on_action(cx.listener(Self::duplicate_line))
            .on_action(cx.listener(Self::save_action))
            .on_action(cx.listener(|e, _: &crate::ShowCompletions, _, cx| e.show_completions(cx)))
            .on_action(
                cx.listener(|e, _: &crate::ShowSignatureHelp, _, cx| e.show_signature_help(cx)),
            )
            .child(EditorElement::new(cx.entity()))
            .children(popover)
    }
}

/// The popover listing the diagnostics under the mouse.
fn hover_popover(hover: &HoverState, cx: &Context<Editor>) -> AnyElement {
    let colors = &theme(cx).colors;
    let ui = &settings(cx).ui;
    let mut body = div()
        .id("diagnostic-popover")
        .occlude()
        .flex()
        .flex_col()
        .gap_1()
        .max_w(px(520.))
        .px_2()
        .py_1p5()
        .bg(colors.surface_raised)
        .border_1()
        .border_color(colors.border)
        .rounded_md()
        .shadow_md()
        .text_size(px(ui.font_size))
        .text_color(colors.text);
    if let Some(family) = &ui.font_family {
        body = body.font_family(SharedString::from(family.clone()));
    }
    for d in &hover.diagnostics {
        let color = d.severity.color(colors);
        let mut row = div()
            .flex()
            .flex_row()
            .items_start()
            .gap_2()
            .child(
                div()
                    .pt(px(2.))
                    .child(icon(d.severity.icon()).size(px(14.)).color(color)),
            )
            .child(div().flex_1().child(SharedString::from(d.message.clone())));
        if !d.source.is_empty() {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(colors.text_muted)
                    .child(SharedString::from(d.source.clone())),
            );
        }
        body = body.child(row);
    }
    deferred(
        anchored()
            .position(hover.anchor)
            .snap_to_window_with_margin(px(8.))
            .child(div().pt(px(2.)).child(body)),
    )
    .with_priority(1)
    .into_any_element()
}
