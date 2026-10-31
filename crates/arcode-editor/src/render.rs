//! `Render` for [`Editor`]: key context, action handlers, the element and
//! the focus check that closes the hover.

use gpui::{
    Context, IntoElement, ModifiersChangedEvent, ParentElement, Render, Styled, Window, div,
    prelude::*,
};

use crate::Editor;
use crate::actions::KEY_CONTEXT;
use crate::element::EditorElement;

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_blink(window, cx);
        if !self.focus_handle.is_focused(window) {
            self.completion_blurred(cx);
        }
        self.hover_check_focus(window, cx);
        self.lang_check_focus(window, cx);
        self.inlay_sync(cx);
        self.vim_sync(cx);
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
            .on_action(cx.listener(Self::select_all_occurrences))
            .on_action(cx.listener(Self::undo_cursor))
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
            .on_action(cx.listener(|e, _: &crate::ShowHover, _, cx| e.show_hover(cx)))
            .on_action(
                cx.listener(|e, _: &crate::ShowCodeActions, _, cx| e.show_code_actions(false, cx)),
            )
            .on_action(cx.listener(|e, _: &crate::Refactor, _, cx| e.show_code_actions(true, cx)))
            .on_action(cx.listener(|e, _: &crate::RenameSymbol, _, cx| e.rename_symbol(cx)))
            .on_action(cx.listener(|e, _: &crate::FormatDocument, _, cx| e.format_document(cx)))
            .on_action(cx.listener(|e, _: &crate::FormatSelection, _, cx| e.format_selection(cx)))
            .on_action(
                cx.listener(|e, _: &crate::ToggleInlayHints, _, cx| e.toggle_inlay_hints(cx)),
            )
            .on_action(cx.listener(|e, _: &crate::ShowContextMenu, window, cx| {
                e.show_context_menu(window, cx)
            }))
            .on_modifiers_changed(cx.listener(|e, ev: &ModifiersChangedEvent, window, cx| {
                e.hover_modifiers_changed(ev.modifiers, window, cx)
            }))
            .child(EditorElement::new(cx.entity()))
    }
}
