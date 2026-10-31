//! The choices of the first-run dialog: theme cards, editor font with a
//! preview line, keymap and performance mode.

use arcode_config::Keymap;
use arcode_ui_kit::theme;
use gpui::{Context, SharedString, div, prelude::*};

use super::first_run::{FirstRunDialog, THEME_CARDS};
use crate::settings_ui::widgets::{segmented, theme_swatch};
use crate::theme_sync::Themes;

impl FirstRunDialog {
    pub(super) fn render_choices(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let themes = cx.global::<Themes>().0.clone();
        let mut cards = div().flex().gap_2();
        for (value, label) in THEME_CARDS {
            let selected = self.choices.theme.as_str().eq_ignore_ascii_case(value);
            let (dark, light) = match value {
                "system" => (
                    themes.get(arcode_config::DEFAULT_DARK_THEME),
                    themes.get(arcode_config::DEFAULT_LIGHT_THEME),
                ),
                name => (themes.get(name), None),
            };
            let value = value.to_owned();
            cards = cards.child(
                theme_swatch(
                    SharedString::from(format!("fr-theme-{value}")),
                    label,
                    dark,
                    light,
                    selected,
                    cx,
                )
                .on_click(cx.listener(move |d, _, window, cx| d.choose_theme(&value, window, cx))),
            );
        }
        let font_ix = self
            .fonts
            .iter()
            .position(|f| *f == self.choices.editor_font)
            .unwrap_or(0);
        let fonts: Vec<&str> = self.fonts.iter().map(String::as_str).collect();
        let keymap_ix = usize::from(self.choices.keymap == Keymap::Vim);
        let perf_ix = usize::from(self.choices.low_end);
        let label = |text: &'static str| {
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(text)
        };
        let this = cx.entity().downgrade();
        let font_names = self.fonts.clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("Theme"))
            .child(cards)
            .child(label("Editor font"))
            .child(segmented("fr-font", &fonts, font_ix, {
                let this = this.clone();
                move |ix, _, cx| {
                    this.update(cx, |d, cx| {
                        if let Some(f) = font_names.get(ix) {
                            d.choices.editor_font = f.clone();
                            cx.notify();
                        }
                    })
                    .ok();
                }
            }, cx))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(c.background)
                    .border_1()
                    .border_color(c.border)
                    .font_family(self.choices.editor_font.clone())
                    .child("fn main() => x != y && a === b // 漢字 🚀"),
            )
            .child(
                div()
                    .flex()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(label("Keymap"))
                            .child(segmented("fr-keymap", &["Default", "Vim"], keymap_ix, {
                                let this = this.clone();
                                move |ix, _, cx| {
                                    this.update(cx, |d, cx| {
                                        d.choices.keymap =
                                            if ix == 1 { Keymap::Vim } else { Keymap::Default };
                                        cx.notify();
                                    })
                                    .ok();
                                }
                            }, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(label("Performance"))
                            .child(segmented(
                                "fr-perf",
                                &["Normal", "Low-end machine"],
                                perf_ix,
                                {
                                    let this = this.clone();
                                    move |ix, _, cx| {
                                        this.update(cx, |d, cx| {
                                            d.choices.low_end = ix == 1;
                                            cx.notify();
                                        })
                                        .ok();
                                    }
                                },
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child("Low-end: no animations or cursor blink, flat popups, no inlay hints or inline blame, slower background refreshes."),
            )
    }
}
