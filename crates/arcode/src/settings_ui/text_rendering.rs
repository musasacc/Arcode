//! Fonts, "Text rendering": `ui.text_gamma` and `ui.text_contrast` (Linux
//! only, applied at startup), a sample of light and dark text drawn as the
//! renderer currently draws them, a note while a restart is needed, and
//! one for each value the renderer takes from the environment instead.

use arcode_config::{DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, TEXT_RENDERING_TUNABLE, UiSettings};
use arcode_ui_kit::metrics::{WEIGHT_MEDIUM, icon_size, type_scale};
use arcode_ui_kit::{IconName, hsla, icon, mono_font_family, settings, theme, ui_font_family};
use gpui::{App, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::{SettingsPage, Steps, group_title};
use crate::theme_sync::Themes;

/// The line the sample shows in each font.
const SAMPLE: &str = "Sphinx of black quartz, judge my vow. 0O 1lI {}[]";

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    if !TEXT_RENDERING_TUNABLE {
        return div();
    }
    let ui = settings(cx).ui.clone();
    let gamma = UiSettings::TEXT_GAMMA_RANGE;
    let contrast = UiSettings::TEXT_CONTRAST_RANGE;
    div()
        .child(group_title("Text rendering", cx))
        .child(page.number_row(
            "Gamma",
            "How text edges blend. Higher draws light text on dark themes heavier, and dark text thinner. Default 1.8. Takes effect after a restart.",
            "ui.text_gamma",
            ui.text_gamma,
            Steps {
                step: 0.1,
                min: *gamma.start(),
                max: *gamma.end(),
                int: false,
            },
            cx,
        ))
        .child(page.number_row(
            "Contrast",
            "Thickens dark and mid-tone text: light themes, muted text, syntax colours. Light text is left as it is. Default 1.0. Takes effect after a restart.",
            "ui.text_contrast",
            ui.text_contrast,
            Steps {
                step: 0.5,
                min: *contrast.start(),
                max: *contrast.end(),
                int: false,
            },
            cx,
        ))
        .when(!super::filter::active(), |el| {
            el.child(sample(cx))
                .when(crate::text_rendering::restart_needed(&ui, cx), |el| {
                    el.child(note(RESTART_NOTE.into(), cx))
                })
                .children(
                    crate::text_rendering::outside_values(cx)
                        .iter()
                        .map(|(name, value)| note(outside_note(name, value), cx))
                        .collect::<Vec<_>>(),
                )
        })
}

/// Light text on Ember and dark text on Chalk, in the UI and the code
/// font, so both settings can be judged whichever theme is in use.
fn sample(cx: &App) -> Div {
    let c = &theme(cx).colors;
    let themes = cx.try_global::<Themes>().map(|t| &t.0);
    let swatch = |name: &str| {
        let ui = &themes?.get(name)?.ui;
        Some((hsla(ui.background), hsla(ui.text), hsla(ui.text_muted)))
    };
    let swatches = [swatch(DEFAULT_DARK_THEME), swatch(DEFAULT_LIGHT_THEME)];
    div().mt_2().flex().flex_col().gap_2().children(
        swatches
            .into_iter()
            .flatten()
            .map(|(bg, text, muted)| sample_box(bg, text, muted, c.border, cx)),
    )
}

fn sample_box(bg: Hsla, text: Hsla, muted: Hsla, border: Hsla, cx: &App) -> Div {
    let scale = type_scale(cx);
    let editor = &settings(cx).editor;
    let line = |family: SharedString, size, color| {
        div()
            .whitespace_nowrap()
            .font_family(family)
            .text_size(size)
            .text_color(color)
            .child(SAMPLE)
    };
    div()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap_1()
        .child(line(ui_font_family(cx), scale.body, text))
        .child(line(ui_font_family(cx), scale.meta, muted))
        .child(
            line(
                SharedString::from(editor.font_family.clone()),
                px(editor.font_size),
                text,
            )
            .font_weight(FontWeight(f32::from(editor.font_weight))),
        )
        .child(line(mono_font_family(), scale.readout, muted))
}

const RESTART_NOTE: &str =
    "Restart Arcode to apply the new text settings; the sample shows the current ones.";

/// The note for a variable set outside Arcode that the renderer uses.
fn outside_note(name: &str, value: &str) -> SharedString {
    let setting = if name == arcode_config::TEXT_GAMMA_ENV {
        "Gamma"
    } else {
        "Contrast"
    };
    format!("{name}={value} is set in the environment and used while {setting} is at its default.")
        .into()
}

fn note(text: SharedString, cx: &App) -> Div {
    let c = &theme(cx).colors;
    div()
        .mt_2()
        .flex()
        .items_center()
        .gap_2()
        .text_size(type_scale(cx).meta)
        .font_weight(WEIGHT_MEDIUM)
        .text_color(c.text)
        .child(icon(IconName::Info).size(icon_size::INLINE).color(c.info))
        .child(text)
}
