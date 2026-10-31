//! Drawing the consent dialog.

use arcode_ui_kit::metrics::{CONTROL_H, CONTROL_RADIUS, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD};
use arcode_ui_kit::{IconName, icon, overlay, settings, theme, type_scale, ui_font_family};
use gpui::{Context, KeyDownEvent, SharedString, Window, div, prelude::*, px};

use super::{ConsentDialog, Control};
use crate::plugins::consent::Choice;

impl ConsentDialog {
    fn render_button(&self, choice: Choice, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let focused = self.focused == Control::Answer(choice);
        let deny = choice == Choice::Deny;
        let armed = self.armed;
        // Every answer is a Secondary outline: a consent dialog nudges
        // towards none of them. The keyboard focus turns the edge `accent`;
        // until armed the buttons read as disabled.
        let edge = if focused {
            c.accent
        } else if armed {
            c.border_strong
        } else {
            c.border
        };
        div()
            .id(SharedString::from(format!("consent-{choice:?}")))
            .flex()
            .flex_none()
            .items_center()
            .h(CONTROL_H)
            .px(px(11.))
            .rounded(CONTROL_RADIUS)
            .font_weight(WEIGHT_MEDIUM)
            .border_1()
            .border_color(edge)
            .cursor_pointer()
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
            .gap(px(8.))
            .px(px(4.))
            .py(px(2.))
            .rounded(px(6.))
            .cursor_pointer()
            .border_1()
            .border_color(if focused {
                c.accent
            } else {
                gpui::transparent_black()
            })
            .child(
                // Checkbox (docs/DESIGN.md 6.13): 14, radius 4.
                div()
                    .size(px(14.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if self.write {
                        c.accent
                    } else {
                        c.border_strong
                    })
                    .when(self.write, |el| {
                        el.bg(c.accent)
                            .child(icon(IconName::Check).size(px(11.)).color(c.accent_text))
                    }),
            )
            .child("Also allow changing files")
            .on_click(cx.listener(|d, _, _, cx| d.press(Control::Write, cx)))
    }
}

impl Render for ConsentDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let meta = scale.meta;
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
            .gap(px(8.))
            .child(label("Plugin"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().child(m.plugin_name.clone()))
                    .child(
                        div()
                            .text_size(meta)
                            .text_color(c.text_muted)
                            .child(format!("{} · {}", m.plugin_id, m.source)),
                    ),
            );
        let what = div()
            .flex()
            .items_start()
            .gap(px(8.))
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
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .bg(c.panel())
                            .border_1()
                            .border_color(c.border)
                            .font_family(mono)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(c.text)
                            .child(m.target.clone()),
                    ),
            );
        let notes = m.notes.iter().map(|n| {
            div()
                .text_size(meta)
                .text_color(c.text_muted)
                .child(n.clone())
        });
        let buttons: Vec<_> = Choice::ALL
            .iter()
            .map(|choice| self.render_button(*choice, cx))
            .collect();
        let write = m.write_option.then(|| self.render_write(cx));
        let dialog = overlay(cx)
            .id("consent-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|d, e: &KeyDownEvent, _, cx| d.on_key(e, cx)))
            .w(px(520.))
            .p(px(16.))
            .gap(px(12.))
            .font_family(ui_font_family(cx))
            .text_size(scale.body)
            .text_color(c.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(IconName::TriangleAlert).size(px(18.)).color(c.warning))
                    .child(
                        div()
                            .text_size(scale.title)
                            .font_weight(WEIGHT_SEMIBOLD)
                            .child(m.title),
                    ),
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
                    .gap(px(8.))
                    .children(buttons),
            )
            .child(
                div()
                    .text_size(meta)
                    .text_color(c.text_muted)
                    .child("Tab moves, Enter answers, Esc decides later (asked again)."),
            );
        // No dim behind dialogs (docs/DESIGN.md 5.2): the layer only blocks
        // the window below.
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(dialog)
    }
}
