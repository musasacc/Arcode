//! Turning one buffer line into a display string and styled text runs.
//!
//! Tabs are expanded to spaces up to the next tab stop. Every char column
//! keeps its display byte offset so cursor positions, selections and mouse
//! hits can be mapped between buffer columns and the shaped line.

use arcode_syntax::HighlightSpan;
use arcode_ui_kit::UiTheme;
use std::ops::Range;

use gpui::{Font, FontStyle, HighlightStyle, Hsla, SharedString, TextRun};

/// Lines longer than this many chars are cut off for display, which keeps
/// shaping cost bounded for minified files.
pub const MAX_DISPLAY_CHARS: usize = 20_000;

/// Virtual text to insert into a display line before char column `col`.
#[derive(Debug, Clone)]
pub struct InlayText<'a> {
    /// Buffer char column it is drawn before (clamped to the line end).
    pub col: usize,
    /// The text.
    pub text: &'a str,
    /// Text color.
    pub color: Hsla,
    /// Background behind the text.
    pub background: Option<Hsla>,
    /// Italic.
    pub italic: bool,
}

/// A buffer line prepared for shaping.
#[derive(Debug, Clone, Default)]
pub struct DisplayLine {
    /// The display text (tabs expanded, inlay text inserted).
    pub text: SharedString,
    /// Display byte offset of the caret before each char column, plus one
    /// entry for the end. Inlay text at a column comes after its caret.
    pub col_to_byte: Vec<usize>,
    /// Styled runs covering `text` exactly.
    pub runs: Vec<TextRun>,
    /// Inlay text: buffer column and display byte range, sorted.
    pub inlays: Vec<(usize, Range<usize>)>,
}

impl DisplayLine {
    /// Display byte offset of the caret at char column `col` (clamped).
    pub fn byte_for_col(&self, col: usize) -> usize {
        let last = self.col_to_byte.len().saturating_sub(1);
        self.col_to_byte.get(col.min(last)).copied().unwrap_or(0)
    }

    /// Display byte where the glyph of char column `col` starts, i.e.
    /// after any inlay text at that column.
    pub fn char_start(&self, col: usize) -> usize {
        let inlay: usize = self
            .inlays
            .iter()
            .filter(|(c, _)| *c == col)
            .map(|(_, r)| r.len())
            .sum();
        self.byte_for_col(col) + inlay
    }

    /// Number of columns whose glyph starts at or before `byte`.
    fn cols_starting_by(&self, byte: usize) -> usize {
        if self.inlays.is_empty() {
            return self.col_to_byte.partition_point(|&b| b <= byte);
        }
        let (mut lo, mut hi) = (0, self.col_to_byte.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.char_start(mid) <= byte {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The inlay column whose text contains display byte `byte`.
    fn inlay_at(&self, byte: usize) -> Option<usize> {
        self.inlays
            .iter()
            .find(|(_, r)| r.contains(&byte))
            .map(|(c, _)| *c)
    }

    /// Char column nearest to display byte `byte` (rounds within tabs;
    /// bytes inside inlay text snap to the inlay's column).
    pub fn col_for_byte(&self, byte: usize) -> usize {
        if let Some(col) = self.inlay_at(byte) {
            return col;
        }
        let col = self.cols_starting_by(byte).saturating_sub(1);
        let a = self.char_start(col);
        match self.col_to_byte.get(col + 1) {
            Some(&b) if byte > a && b > a && (byte - a) * 2 >= b - a => col + 1,
            _ => col,
        }
    }

    /// Char column whose display bytes contain `byte` (rounds down).
    pub fn col_at_byte(&self, byte: usize) -> usize {
        if let Some(col) = self.inlay_at(byte) {
            return col;
        }
        self.cols_starting_by(byte).saturating_sub(1)
    }
}

/// Expands `line` (no line break), inserts `inlays` (sorted by column)
/// and maps highlight `spans` (byte ranges of `line`) to runs styled from
/// `theme`.
pub fn build_display_line(
    line: &str,
    spans: &[HighlightSpan],
    inlays: &[InlayText<'_>],
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
    let mut inlay_ranges: Vec<(usize, Range<usize>)> = Vec::new();
    let mut pending = inlays.iter().peekable();
    let mut push_inlays = |col: usize, text: &mut String, last: bool| {
        while let Some(inlay) = pending.next_if(|i| i.col <= col || last) {
            let start = text.len();
            text.push_str(inlay.text);
            inlay_ranges.push((col, start..text.len()));
        }
    };
    for (col, (src_byte, c)) in line.char_indices().take(MAX_DISPLAY_CHARS).enumerate() {
        while src_to_display.len() < src_byte {
            src_to_display.push(text.len());
        }
        col_to_byte.push(text.len());
        push_inlays(col, &mut text, false);
        src_to_display.push(text.len());
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
    let end_col = col_to_byte.len() - 1;
    push_inlays(end_col, &mut text, true);

    let map = |b: usize| src_to_display[b.min(src_len)];
    let default = HighlightStyle {
        color: Some(theme.colors.text),
        ..Default::default()
    };
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len() * 2 + 1);
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
    if !inlay_ranges.is_empty() {
        let styled = inlay_ranges.iter().zip(inlays).map(|((_, r), inlay)| {
            let mut f = font.clone();
            if inlay.italic {
                f.style = FontStyle::Italic;
            }
            let run = TextRun {
                len: r.len(),
                font: f,
                color: inlay.color,
                background_color: inlay.background,
                underline: None,
                strikethrough: None,
            };
            (r.clone(), run)
        });
        runs = overlay_runs(runs, styled.collect());
    }
    DisplayLine {
        text: text.into(),
        col_to_byte,
        runs,
        inlays: inlay_ranges,
    }
}

/// Replaces the parts of `runs` covered by `overlays` (sorted, disjoint
/// display ranges) with the overlay runs.
fn overlay_runs(runs: Vec<TextRun>, overlays: Vec<(Range<usize>, TextRun)>) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + overlays.len() * 2);
    let mut overlays = overlays.into_iter().peekable();
    let mut pos = 0;
    for run in runs {
        let end = pos + run.len;
        let mut at = pos;
        while let Some((r, o)) = overlays.next_if(|(r, _)| r.start < end) {
            if r.start > at {
                out.push(TextRun {
                    len: r.start - at,
                    ..run.clone()
                });
            }
            out.push(o);
            at = r.end;
        }
        if end > at {
            out.push(TextRun {
                len: end - at,
                ..run.clone()
            });
        }
        pos = end;
    }
    out
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
        let line = build_display_line("\tab\tc", &[], &[], 4, &gpui::font("Mono"), &t);
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
        let line = build_display_line("fn main()", &spans, &[], 4, &gpui::font("Mono"), &t);
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
        let line = build_display_line("\t\"é\"x", &spans, &[], 2, &gpui::font("Mono"), &t);
        assert_eq!(line.text.as_ref(), "  \"é\"x");
        let lens: Vec<usize> = line.runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![2, 4, 1]);
        assert_eq!(line.col_to_byte.len(), 6);
    }

    #[test]
    fn inlays_are_skipped_by_columns() {
        let t = theme();
        let muted = t.colors.text_muted;
        let inlay = |col, text| InlayText {
            col,
            text,
            color: muted,
            background: None,
            italic: false,
        };
        let spans = vec![HighlightSpan {
            range: 0..3,
            name: "keyword",
        }];
        let inlays = [inlay(1, ": T"), inlay(3, "<-")];
        let line = build_display_line("let", &spans, &inlays, 4, &gpui::font("Mono"), &t);
        assert_eq!(line.text.as_ref(), "l: Tet<-");
        // Carets sit before inlay text; glyphs start after it.
        assert_eq!(line.col_to_byte, vec![0, 1, 5, 6]);
        assert_eq!(line.char_start(1), 4);
        assert_eq!(line.char_start(3), 8);
        // Clicking inside inlay text snaps to its column.
        assert_eq!(line.col_for_byte(2), 1);
        assert_eq!(line.col_for_byte(7), 3);
        assert_eq!(line.col_at_byte(4), 1);
        assert_eq!(line.col_at_byte(5), 2);
        let lens: Vec<usize> = line.runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![1, 3, 2, 2]);
        assert_eq!(line.runs[1].color, muted);
        assert_eq!(line.inlays, vec![(1, 1..4), (3, 6..8)]);
    }
}
