//! Rasterizing minimap lines: each char becomes its glyph pattern (see
//! [`super::glyphs`]) in its syntax colour, into a BGRA pixel buffer with
//! straight alpha, the format GPUI images use.

use arcode_syntax::HighlightSpan;
use arcode_ui_kit::UiTheme;
use gpui::{Hsla, Rgba};

use super::glyphs::{Patterns, shape_of};

/// Opacity of the glyphs: the minimap stays quieter than the code.
pub(crate) const GLYPH_OPACITY: f32 = 0.8;

/// A colour as BGRA bytes with straight alpha.
pub(crate) type Bgra = [u8; 4];

/// `color` at `opacity` as BGRA bytes.
pub(crate) fn bgra(color: Hsla, opacity: f32) -> Bgra {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    [byte(c.b), byte(c.g), byte(c.r), byte(c.a * opacity)]
}

/// Glyph colours by capture name for one raster pass. Capture names are
/// static strings from the grammar's query, so a pointer comparison finds
/// them.
pub(crate) struct ColorCache<'a> {
    theme: &'a UiTheme,
    known: Vec<(&'static str, Bgra)>,
    pub default: Bgra,
}

impl<'a> ColorCache<'a> {
    pub fn new(theme: &'a UiTheme) -> Self {
        ColorCache {
            theme,
            known: Vec::new(),
            default: bgra(theme.colors.text, GLYPH_OPACITY),
        }
    }

    pub fn get(&mut self, name: &'static str) -> Bgra {
        if let Some((_, c)) = self.known.iter().find(|(n, _)| std::ptr::eq(*n, name)) {
            return *c;
        }
        let color = self
            .theme
            .syntax(name)
            .color
            .unwrap_or(self.theme.colors.text);
        let color = bgra(color, GLYPH_OPACITY);
        self.known.push((name, color));
        color
    }
}

/// One line's pixel rows in a BGRA buffer: `pitch` rows of `stride` bytes
/// starting at `offset`.
pub(crate) struct LineTarget<'b> {
    pub pixels: &'b mut [u8],
    pub stride: usize,
    pub offset: usize,
}

/// Draws one line (`chars`, with highlight `spans` as byte ranges of the
/// line) into `target`. Tabs advance to the next tab stop; columns past
/// `max_cols` are dropped.
pub(crate) fn raster_line(
    chars: impl Iterator<Item = char>,
    spans: &[HighlightSpan],
    tab_size: usize,
    max_cols: usize,
    colors: &mut ColorCache<'_>,
    patterns: &Patterns,
    target: &mut LineTarget<'_>,
) {
    let tab_size = tab_size.max(1);
    let (cw, pitch) = (patterns.col, patterns.pitch);
    let mut col = 0usize;
    let mut byte = 0usize;
    let mut span_ix = 0usize;
    // The colour of span `span_ix`, looked up once per span.
    let mut span_color: Option<(usize, Bgra)> = None;
    for c in chars {
        if col >= max_cols || c == '\n' || c == '\r' {
            break;
        }
        let width = if c == '\t' {
            tab_size - col % tab_size
        } else {
            1
        };
        if let Some((shape, narrow)) = shape_of(c) {
            while spans.get(span_ix).is_some_and(|s| s.range.end <= byte) {
                span_ix += 1;
            }
            let color = match spans.get(span_ix) {
                Some(s) if s.range.start <= byte => match span_color {
                    Some((ix, c)) if ix == span_ix => c,
                    _ => {
                        let c = colors.get(s.name);
                        span_color = Some((span_ix, c));
                        c
                    }
                },
                _ => colors.default,
            };
            let cell = patterns.get(shape, narrow);
            let x = col * cw * 4;
            for r in 0..pitch {
                let row = target.offset + r * target.stride + x;
                for (i, &coverage) in cell[r * cw..(r + 1) * cw].iter().enumerate() {
                    if coverage == 0 {
                        continue;
                    }
                    let px = &mut target.pixels[row + i * 4..row + i * 4 + 4];
                    let a = (u32::from(color[3]) * u32::from(coverage) / 255) as u8;
                    px.copy_from_slice(&[color[0], color[1], color[2], a]);
                }
            }
        }
        col += width;
        byte += c.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;
    use crate::minimap::glyphs::Shape;

    fn theme() -> UiTheme {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        UiTheme::new(std::sync::Arc::new(theme))
    }

    fn span(range: Range<usize>, name: &'static str) -> HighlightSpan {
        HighlightSpan { range, name }
    }

    /// Rasterizes `line` into a buffer `max_cols` cells wide and returns
    /// the buffer and its stride.
    fn raster(line: &str, spans: &[HighlightSpan], max_cols: usize, p: &Patterns) -> Vec<u8> {
        let theme = theme();
        let mut colors = ColorCache::new(&theme);
        let stride = max_cols * p.col * 4;
        let mut pixels = vec![0; stride * p.pitch];
        let mut target = LineTarget {
            pixels: &mut pixels,
            stride,
            offset: 0,
        };
        raster_line(
            line.chars(),
            spans,
            4,
            max_cols,
            &mut colors,
            p,
            &mut target,
        );
        pixels
    }

    /// The pixel at cell column `col`, pixel row `row` (1x cells).
    fn at(pixels: &[u8], max_cols: usize, col: usize, row: usize) -> Bgra {
        let i = row * max_cols * 4 + col * 4;
        pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn chars_take_their_colours_and_outlines() {
        let theme = theme();
        let p = Patterns::new(1, 2);
        let spans = [span(4..6, "keyword"), span(7..11, "function")];
        let px = raster("    fn main() {}", &spans, 20, &p);
        let keyword = bgra(theme.syntax("keyword").color.unwrap(), GLYPH_OPACITY);
        let function = bgra(theme.syntax("function").color.unwrap(), GLYPH_OPACITY);
        let text = bgra(theme.colors.text, GLYPH_OPACITY);
        // Indentation and the gap between words stay empty.
        for col in [0, 3, 6, 13] {
            assert_eq!(at(&px, 20, col, 1)[3], 0, "column {col}");
        }
        // 'f' is narrow and tall, 'n' short: the lower row carries both.
        assert_eq!(at(&px, 20, 4, 1)[..3], keyword[..3]);
        assert_eq!(at(&px, 20, 5, 1), keyword, "'n', full lower row");
        let short = Patterns::new(1, 2).get(Shape::Short, false)[0];
        assert_eq!(
            u32::from(at(&px, 20, 5, 0)[3]),
            u32::from(keyword[3]) * u32::from(short) / 255
        );
        assert_eq!(at(&px, 20, 7, 1), function);
        assert_eq!(at(&px, 20, 11, 1)[..3], text[..3], "'(' plain");
    }

    #[test]
    fn tabs_expand_and_long_lines_are_cut() {
        let p = Patterns::new(1, 2);
        let px = raster("\tx\ty", &[], 12, &p);
        let lit: Vec<usize> = (0..12).filter(|&c| at(&px, 12, c, 1)[3] > 0).collect();
        assert_eq!(lit, vec![4, 8]);
        // 500 chars into 10 columns: nothing is written past them.
        let px = raster(&"a".repeat(500), &[], 10, &p);
        assert!((0..10).all(|c| at(&px, 10, c, 1)[3] > 0));
        assert!(raster("   \r\n", &[], 10, &p).iter().all(|&b| b == 0));
    }

    #[test]
    fn multibyte_chars_keep_spans_aligned() {
        let theme = theme();
        let p = Patterns::new(1, 2);
        // "é" is two bytes: the string span starts at byte 4, column 3.
        let px = raster("é = \"ab\"", &[span(4..9, "string")], 10, &p);
        let string = bgra(theme.syntax("string").color.unwrap(), GLYPH_OPACITY);
        assert_eq!(at(&px, 10, 5, 1), string, "'a' inside the string");
        assert_ne!(at(&px, 10, 0, 1)[..3], string[..3]);
    }

    #[test]
    fn two_x_cells_are_two_pixels_wide() {
        let p = Patterns::new(2, 4);
        let px = raster("ab", &[], 2, &p);
        let stride = 2 * 2 * 4;
        let alpha = |x: usize, y: usize| px[y * stride + x * 4 + 3];
        // 'a' is short: empty top row, x-height rows; 'b' tall.
        assert_eq!((alpha(0, 0), alpha(1, 0)), (0, 0));
        assert!(alpha(0, 1) > 0 && alpha(1, 2) > 0);
        assert!(alpha(2, 0) > 0 && alpha(3, 0) > 0);
        // The gap row between lines stays empty.
        assert!((0..4).all(|x| alpha(x, 3) == 0));
    }
}
