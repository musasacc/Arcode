//! Fonts: editor and UI font families (pickers over the system fonts plus
//! the bundled JetBrains Mono), sizes, line height, ligatures and a live
//! preview with ligature, CJK and emoji samples.

use std::sync::Arc;

use arcode_config::EditorSettings;
use arcode_ui_kit::{settings, theme, type_scale};
use gpui::{App, Context, Div, FontFallbacks, FontFeatures, SharedString, div, prelude::*, px};

use super::widgets::{picker_button, setting_row};
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
    let c = theme(cx).colors.clone();
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
        .child(setting_row(
            "Family",
            "System fonts and the bundled JetBrains Mono. The picker previews as you move.",
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
        .child(setting_row(
            "Fallback fonts",
            "Tried in order for characters the font lacks (CJK, emoji). Edit them in config.toml.",
            div()
                .max_w(px(260.))
                .text_size(type_scale(cx).meta)
                .text_color(c.text_muted)
                .child(e.font_fallbacks.join(", ")),
            cx,
        ))
        .child(font_preview(e, cx))
        .child(group_title("Interface font", cx))
        .child(setting_row("Family", "", ui_font, cx))
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
}
