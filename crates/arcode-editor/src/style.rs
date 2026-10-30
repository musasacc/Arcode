//! Editor style derived from the settings global.

use arcode_buffer::ops::{AutoClose, IndentUnit, OpsConfig};
use arcode_ui_kit::settings;
use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, px};

/// Font and metrics the editor draws with.
#[derive(Debug, Clone)]
pub struct EditorStyle {
    /// Base font (family, fallbacks, ligature features).
    pub font: Font,
    /// Font size.
    pub font_size: Pixels,
    /// Height of one line.
    pub line_height: Pixels,
    /// Display width of a tab.
    pub tab_size: usize,
    /// Draw the current-line highlight.
    pub highlight_current_line: bool,
    /// Show line numbers in the gutter.
    pub show_line_numbers: bool,
    /// Blink the cursor while focused.
    pub cursor_blink: bool,
}

impl EditorStyle {
    /// Reads the style from settings, with per-language tab size.
    pub fn from_settings(language_id: &str, cx: &App) -> Self {
        let config = settings(cx);
        let ed = &config.editor;
        let font_size = px(ed.font_size.clamp(4.0, 128.0));
        let fallbacks: Vec<String> = ed
            .font_fallbacks
            .iter()
            .filter(|f| **f != ed.font_family)
            .cloned()
            .collect();
        let font = Font {
            family: SharedString::from(ed.font_family.clone()),
            features: if ed.ligatures {
                FontFeatures::default()
            } else {
                FontFeatures::disable_ligatures()
            },
            fallbacks: (!fallbacks.is_empty()).then(|| FontFallbacks::from_fonts(fallbacks)),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        EditorStyle {
            font,
            font_size,
            line_height: (font_size * ed.line_height.clamp(1.0, 3.0)).round(),
            tab_size: config.effective_tab_size(language_id).clamp(1, 16),
            highlight_current_line: ed.highlight_current_line,
            show_line_numbers: ed.show_line_numbers,
            cursor_blink: ed.cursor_blink,
        }
    }
}

/// Editing-command settings for a language.
pub fn ops_config(language_id: &str, cx: &App) -> OpsConfig {
    let config = settings(cx);
    let tab = config.effective_tab_size(language_id).clamp(1, 16);
    OpsConfig {
        indent: if config.effective_insert_spaces(language_id) {
            IndentUnit::Spaces(tab)
        } else {
            IndentUnit::Tab
        },
        tab_width: tab,
        auto_close: if config.editor.auto_close_brackets {
            AutoClose::default()
        } else {
            AutoClose::disabled()
        },
    }
}
