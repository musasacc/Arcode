//! Turning one buffer line into a display string and styled text runs.
//!
//! Tabs are expanded to spaces up to the next tab stop. Every char column
//! keeps its display byte offset so cursor positions, selections and mouse
//! hits can be mapped between buffer columns and the shaped line.

use arcode_syntax::HighlightSpan;
use arcode_ui_kit::UiTheme;
use gpui::{Font, HighlightStyle, Hsla, SharedString, TextRun};

/// Lines longer than this many chars are cut off for display, which keeps
/// shaping cost bounded for minified files.
pub const MAX_DISPLAY_CHARS: usize = 20_000;

/// A buffer line prepared for shaping.
#[derive(Debug, Clone, Default)]
pub struct DisplayLine {
    /// The display text (tabs expanded).
    pub text: SharedString,
    /// Display byte offset of each char column, plus one entry for the end.
    pub col_to_byte: Vec<usize>,
    /// Styled runs covering `text` exactly.
    pub runs: Vec<TextRun>,
}

impl DisplayLine {
    /// Display byte offset of char column `col` (clamped to the end).
    pub fn byte_for_col(&self, col: usize) -> usize {
        let last = self.col_to_byte.len().saturating_sub(1);
        self.col_to_byte.get(col.min(last)).copied().unwrap_or(0)
    }

    /// Char column nearest to display byte `byte` (rounds within tabs).
    pub fn col_for_byte(&self, byte: usize) -> usize {
        let map = &self.col_to_byte;
        let col = map.partition_point(|&b| b <= byte).saturating_sub(1);
        match (map.get(col), map.get(col + 1)) {
            (Some(&a), Some(&b)) if byte > a && (byte - a) * 2 >= b - a => col + 1,
            _ => col,
        }
    }

    /// Char column whose display bytes contain `byte` (rounds down).
    pub fn col_at_byte(&self, byte: usize) -> usize {
        self.col_to_byte
            .partition_point(|&b| b <= byte)
            .saturating_sub(1)
    }
}

/// Expands `line` (no line break) and maps highlight `spans` (byte ranges
/// of `line`) to runs styled from `theme`.
pub fn build_display_line(
    line: &str,
    spans: &[HighlightSpan],
    tab_size: usize,
    font: &Font,
    theme: &UiTheme,
) -> DisplayLine {
    let tab_size = tab_size.max(1);
    let mut text = String::with_capacity(line.len());
    let mut col_to_byte = Vec::with_capacity(line.len() + 1);
    // Display byte offset for each source byte that starts a char.
    let mut src_to_display = Vec::with_capacity(line.len() + 1);
    let mut display_col = 0usize;
    for (src_byte, c) in line.char_indices().take(MAX_DISPLAY_CHARS) {
        while src_to_display.len() < src_byte {
            src_to_display.push(text.len());
        }
        src_to_display.push(text.len());
        col_to_byte.push(text.len());
        if c == '\t' {
            let n = tab_size - display_col % tab_size;
            text.extend(std::iter::repeat_n(' ', n));
            display_col += n;
        } else if c.is_control() {
            text.push(' ');
            display_col += 1;
        } else {
            text.push(c);
            display_col += 1;
        }
    }
    let src_len = line
        .char_indices()
        .nth(MAX_DISPLAY_CHARS)
        .map_or(line.len(), |(i, _)| i);
    while src_to_display.len() <= src_len {
        src_to_display.push(text.len());
    }
    col_to_byte.push(text.len());

    let map = |b: usize| src_to_display[b.min(src_len)];
    let default = HighlightStyle {
        color: Some(theme.colors.text),
        ..Default::default()
    };
    let mut runs = Vec::with_capacity(spans.len() * 2 + 1);
    let mut pos = 0;
    for span in spans {
        let start = map(span.range.start).max(pos);
        let end = map(span.range.end);
        if start >= end {
            continue;
        }
        if start > pos {
            runs.push(text_run(start - pos, font, &default, theme.colors.text));
        }
        let style = theme.syntax(span.name);
        runs.push(text_run(end - start, font, &style, theme.colors.text));
        pos = end;
    }
    if pos < text.len() || runs.is_empty() {
        runs.push(text_run(
            text.len() - pos,
            font,
            &default,
            theme.colors.text,
        ));
    }
    DisplayLine {
        text: text.into(),
        col_to_byte,
        runs,
    }
}

/// A [`TextRun`] of `len` bytes styled with `style` on top of `font`.
pub fn text_run(len: usize, font: &Font, style: &HighlightStyle, fallback: Hsla) -> TextRun {
    let mut font = font.clone();
    if let Some(weight) = style.font_weight {
        font.weight = weight;
    }
    if let Some(font_style) = style.font_style {
        font.style = font_style;
    }
    let mut color = style.color.unwrap_or(fallback);
    if let Some(fade) = style.fade_out {
        color.fade_out(fade);
    }
    TextRun {
        len,
        font,
        color,
        background_color: style.background_color,
        underline: style.underline,
        strikethrough: style.strikethrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> UiTheme {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry
            .get("Arcode Dark")
            .cloned()
            .unwrap_or_else(|| registry.fallback(arcode_config::Appearance::Dark).clone());
        UiTheme::new(std::sync::Arc::new(theme))
    }

    #[test]
    fn expands_tabs_and_maps_columns() {
        let t = theme();
        let line = build_display_line("\tab\tc", &[], 4, &gpui::font("Mono"), &t);
        assert_eq!(line.text.as_ref(), "    ab  c");
        assert_eq!(line.col_to_byte, vec![0, 4, 5, 6, 8, 9]);
        assert_eq!(line.col_for_byte(1), 0);
        assert_eq!(line.col_for_byte(3), 1);
        assert_eq!(line.col_for_byte(4), 1);
        assert_eq!(line.col_for_byte(99), 5);
        assert_eq!(line.byte_for_col(99), 9);
        let total: usize = line.runs.iter().map(|r| r.len).sum();
        assert_eq!(total, line.text.len());
    }

    #[test]
    fn spans_become_runs() {
        let t = theme();
        let spans = vec![
            HighlightSpan {
                range: 0..2,
                name: "keyword",
            },
            HighlightSpan {
                range: 3..7,
                name: "function",
            },
        ];
        let line = build_display_line("fn main()", &spans, 4, &gpui::font("Mono"), &t);
        let lens: Vec<usize> = line.runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![2, 1, 4, 2]);
    }

    #[test]
    fn multibyte_spans() {
        let t = theme();
        let spans = vec![HighlightSpan {
            range: 1..5,
            name: "string",
        }];
        let line = build_display_line("\t\"é\"x", &spans, 2, &gpui::font("Mono"), &t);
        assert_eq!(line.text.as_ref(), "  \"é\"x");
        let lens: Vec<usize> = line.runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![2, 4, 1]);
        assert_eq!(line.col_to_byte.len(), 6);
    }
}
