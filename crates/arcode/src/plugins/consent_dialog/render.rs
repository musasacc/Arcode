//! Drawing the consent dialog.

use arcode_ui_kit::Elevated;
use arcode_ui_kit::{IconName, icon, settings, theme};
use gpui::{Context, KeyDownEvent, SharedString, Window, div, prelude::*, px};

use super::{ConsentDialog, Control};
use crate::plugins::consent::Choice;

impl ConsentDialog {
    fn render_button(&self, choice: Choice, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let focused = self.focused == Control::Answer(choice);
        let deny = choice == Choice::Deny;
        let armed = self.armed;
        div()
            .id(SharedString::from(format!("consent-{choice:?}")))
            .flex()
            .items_center()
            .h(px(28.))
            .px_3()
            .rounded_md()
            .text_sm()
            .border_1()
            .cursor_pointer()
            .when(focused, |el| {
                el.border_color(c.border_focused).bg(c.selection)
            })
            .when(!focused, |el| el.border_color(c.border).bg(c.surface))
            .text_color(if !armed {
                c.text_disabled
            } else if deny {
                c.error
            } else {
                c.text
            })
            .hover(move |s| s.bg(c.hover))
            .child(choice.label(self.model.has_project))
            .on_click(cx.listener(move |d, _, _, cx| d.press(Control::Answer(choice), cx)))
    }

    fn render_write(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let focused = self.focused == Control::Write;
        div()
            .id("consent-write")
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .rounded_sm()
            .text_sm()
            .cursor_pointer()
            .border_1()
            .border_color(if focused {
                c.border_focused
            } else {
                gpui::transparent_black()
            })
            .child(
                div()
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .border_1()
                    .border_color(c.border)
                    .bg(if self.write { c.accent } else { c.surface })
                    .when(self.write, |el| {
                        el.child(icon(IconName::Check).size(px(11.)).color(c.accent_text))
                    }),
            )
            .child("Also allow changing files")
            .on_click(cx.listener(|d, _, _, cx| d.press(Control::Write, cx)))
    }
}

impl Render for ConsentDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let mono = settings(cx).editor.font_family.clone();
        let m = self.model.clone();
        let label = |text: &'static str| {
            div()
                .flex_none()
                .w(px(64.))
                .text_color(c.text_muted)
                .child(text)
        };
        let who = div()
            .flex()
            .items_start()
            .gap_2()
            .text_sm()
            .child(label("Plugin"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().child(m.plugin_name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(c.text_muted)
                            .child(format!("{} · {}", m.plugin_id, m.source)),
                    ),
            );
        let what = div()
            .flex()
            .items_start()
            .gap_2()
            .text_sm()
            .child(label("Asks to"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .min_w_0()
                    .child(m.action.clone())
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(c.background)
                            .border_1()
                            .border_color(c.border)
                            .font_family(mono)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(c.text)
                            .child(m.target.clone()),
                    ),
            );
        let notes = m
            .notes
            .iter()
            .map(|n| div().text_xs().text_color(c.text_muted).child(n.clone()));
        let buttons: Vec<_> = Choice::ALL
            .iter()
            .map(|choice| self.render_button(*choice, cx))
            .collect();
        let write = m.write_option.then(|| self.render_write(cx));
        let dialog = div()
            .id("consent-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|d, e: &KeyDownEvent, _, cx| d.on_key(e, cx)))
            .w(px(520.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .text_color(c.text)
            .elevated(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(IconName::TriangleAlert).size(px(18.)).color(c.warning))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(m.title)),
            )
            .child(who)
            .child(what)
            .child(div().flex().flex_col().gap_0p5().children(notes))
            .children(write)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .children(buttons),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c.text_disabled)
                    .child("Tab moves, Enter answers, Esc decides later (asked again)."),
            );
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.35))
            .occlude()
            .child(dialog)
    }
}
