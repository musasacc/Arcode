//! Font and cell metrics of the terminal grid.

use arcode_ui_kit::settings;
use gpui::{App, Font, FontFallbacks, FontFeatures, Pixels, Window, font, px};

/// Font and cell metrics for one frame.
pub(crate) struct Metrics {
    pub font: Font,
    pub font_size: Pixels,
    pub cell_width: Pixels,
    pub line_height: Pixels,
}

/// Terminal line height relative to the font size. The editor's spacing
/// is honored but capped so box-drawing characters still connect.
const MAX_LINE_HEIGHT: f32 = 1.4;

pub(crate) fn metrics(window: &Window, cx: &App) -> Metrics {
    let config = settings(cx);
    let font_size = px(config.terminal_font_size());
    let mut font = font(config.terminal_font_family().to_owned());
    if !config.editor.font_fallbacks.is_empty() {
        font.fallbacks = Some(FontFallbacks::from_fonts(
            config.editor.font_fallbacks.clone(),
        ));
    }
    if !config.editor.ligatures {
        font.features = FontFeatures::disable_ligatures();
    }
    let text = window.text_system();
    let id = text.resolve_font(&font);
    let cell_width = text
        .advance(id, font_size, 'm')
        .map(|s| s.width)
        .or_else(|_| text.em_advance(id, font_size))
        .unwrap_or(font_size * 0.6);
    let factor = config.editor.line_height.clamp(1.0, MAX_LINE_HEIGHT);
    Metrics {
        font,
        font_size,
        cell_width: if cell_width > px(0.) {
            cell_width
        } else {
            font_size * 0.6
        },
        line_height: (font_size * factor).round().max(px(1.)),
    }
}
