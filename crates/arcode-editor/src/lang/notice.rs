//! Short notices at the cursor ("No code actions available", a rename
//! error). They close with the next key, click, cursor move or edit.

use arcode_ui_kit::{IconName, icon, popover, theme, type_scale, ui_font_family};
use gpui::{
    Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, prelude::*, px,
};

use super::NOTICE_POPOVER;
use crate::Editor;
use crate::ext::EditorPopover;

/// The notice view.
pub(crate) struct Notice {
    text: SharedString,
}

impl Render for Notice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        popover(cx)
            .id("editor-notice")
            .occlude()
            .max_w(px(480.))
            .px(px(10.))
            .py(px(6.))
            .flex_row()
            .items_center()
            .gap(px(8.))
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(colors.text)
            .child(icon(IconName::Info).size(px(14.)).color(colors.info))
            .child(self.text.clone())
    }
}

impl Editor {
    /// Shows `text` in a small popover at the cursor until the next key,
    /// click, cursor move or edit.
    pub fn show_notice(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.hide_hover();
        let anchor = self.offset_to_point(self.selections.primary().head);
        let view = cx.new(|_| Notice { text });
        let popover = EditorPopover::new(anchor, view.into()).on_key(|editor, _, _, cx| {
            editor.update(cx, |e, cx| e.hide_popover(NOTICE_POPOVER, cx));
            false
        });
        self.show_popover(NOTICE_POPOVER, popover, cx);
    }

    /// The shown notice text, if any.
    pub fn notice_text(&self, cx: &gpui::App) -> Option<String> {
        let view = self.popover(NOTICE_POPOVER)?.content.clone();
        let notice = view.downcast::<Notice>().ok()?;
        Some(notice.read(cx).text.to_string())
    }
}
