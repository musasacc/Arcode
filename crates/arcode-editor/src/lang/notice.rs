//! Short notices at the cursor ("No code actions available", a rename
//! error). They close with the next key, click, cursor move or edit.

use arcode_ui_kit::{IconName, icon, settings, theme};
use gpui::{
    Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, prelude::*, px,
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
        let ui = &settings(cx).ui;
        div()
            .id("editor-notice")
            .occlude()
            .max_w(px(480.))
            .px_2()
            .py_1()
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .bg(colors.surface_raised)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_md()
            .text_size(px(ui.font_size))
            .text_color(colors.text)
            .when_some(ui.font_family.clone(), |d, f| {
                d.font_family(SharedString::from(f))
            })
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
