//! Fonts: editor and UI font families (pickers over the system fonts plus
//! the bundled JetBrains Mono), sizes, weight, line height, ligatures and a
//! live preview with ligature, CJK and emoji samples, then the text
//! rendering settings ([`super::text_rendering`]).

use std::sync::Arc;

use arcode_config::EditorSettings;
use arcode_ui_kit::{settings, theme};
use gpui::{
    App, Context, Div, FontFallbacks, FontFeatures, FontWeight, SharedString, div, prelude::*, px,
};

use super::widgets::picker_button;
use super::{SettingsEvent, SettingsPage, Steps, group_title};
use crate::prefs::pickers::{FontTarget, ui_font_label};

/// Lines of the preview: code with ligature pairs, then CJK and emoji.
pub(crate) const PREVIEW_LINES: [&str; 4] = [
    "fn main() -> Result<(), Error> {",
    "    if a => b != c && x === y || p <= q { /* ligatures */ }",
    "    let greeting = \"你好, こんにちは, 안녕하세요\";",
    "    println!(\"{greeting} 🚀 ✨\"); // 0O 1lI",
];

/// The OpenType features the editor uses: contextual alternates and
/// standard ligatures on, or both off.
pub(crate) fn ligature_features(on: bool) -> FontFeatures {
    let value = u32::from(on);
    FontFeatures(Arc::new(vec![
        ("calt".into(), value),
        ("liga".into(), value),
    ]))
}

/// A box showing [`PREVIEW_LINES`] with the editor font settings.
pub(crate) fn font_preview(e: &EditorSettings, cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    let mut text = div()
        .font_family(SharedString::from(e.font_family.clone()))
        .text_size(px(e.font_size))
        .font_weight(FontWeight(f32::from(e.font_weight)))
        .line_height(px((e.font_size * e.line_height).round()))
        .text_color(c.text);
    let style = text.text_style().get_or_insert_with(Default::default);
    style.font_features = Some(ligature_features(e.ligatures));
    style.font_fallbacks = Some(FontFallbacks::from_fonts(e.font_fallbacks.clone()));
    for line in PREVIEW_LINES {
        text = text.child(div().whitespace_nowrap().child(line));
    }
    div()
        .mt_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(c.border)
        .bg(c.surface)
        .overflow_hidden()
        .child(text)
}

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let e = &config.editor;
    let pick = |target: FontTarget, value: String, cx: &mut Context<SettingsPage>| {
        picker_button(
            SharedString::from(format!("pick-{}", target.key())),
            value,
            cx.listener(move |_, _, _, cx| cx.emit(SettingsEvent::PickFont(target))),
            cx,
        )
    };
    let editor_font = pick(FontTarget::Editor, e.font_family.clone(), cx);
    let ui_font = pick(FontTarget::Ui, ui_font_label(&config), cx);
    div()
        .child(group_title("Editor font", cx))
        .child(page.keyed_row(
            "Family",
            "System fonts and the bundled JetBrains Mono. The picker previews as you move.",
            "editor.font_family",
            editor_font,
            cx,
        ))
        .child(page.number_row(
            "Size",
            "",
            "editor.font_size",
            e.font_size,
            Steps {
                step: 1.,
                min: 6.,
                max: 72.,
                int: false,
            },
            cx,
        ))
        .child(page.number_row(
            "Weight",
            "400 is regular, 700 bold. A font without that weight uses its nearest one; the bundled JetBrains Mono has 400 and 700.",
            "editor.font_weight",
            f32::from(e.font_weight),
            Steps {
                step: 100.,
                min: f32::from(*EditorSettings::FONT_WEIGHTS.start()),
                max: f32::from(*EditorSettings::FONT_WEIGHTS.end()),
                int: true,
            },
            cx,
        ))
        .child(page.number_row(
            "Line height",
            "A multiple of the font size.",
            "editor.line_height",
            e.line_height,
            Steps {
                step: 0.1,
                min: 0.8,
                max: 3.,
                int: false,
            },
            cx,
        ))
        .child(page.bool_row(
            "Ligatures",
            "Join pairs like => != === into one glyph (font features calt and liga).",
            "editor.ligatures",
            e.ligatures,
            cx,
        ))
        .child(page.text_row(
            "Fallback fonts",
            "Tried in order for characters the font lacks (CJK, emoji), separated by commas.",
            "editor.font_fallbacks",
            cx,
        ))
        .child(font_preview(e, cx))
        .child(group_title("Interface font", cx))
        .child(page.keyed_row(
            "Family",
            "Unset: the bundled Inter.",
            "ui.font_family",
            ui_font,
            cx,
        ))
        .child(page.number_row(
            "Size",
            "",
            "ui.font_size",
            config.ui.font_size,
            Steps {
                step: 1.,
                min: 8.,
                max: 32.,
                int: false,
            },
            cx,
        ))
        .child(super::text_rendering::render(page, cx))
}
