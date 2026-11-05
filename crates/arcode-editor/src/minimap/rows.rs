//! Minimap rows: each buffer line as a few horizontal blocks, one per run of
//! non-blank chars that share a syntax colour, one pixel per column.
//!
//! The blocks of the lines around the minimap's view are kept between
//! frames and rebuilt only when the text, the highlights, the theme or the
//! range in view change, so an idle redraw (a cursor blink, a hover) costs
//! nothing but painting.

use std::ops::Range;

use arcode_buffer::Buffer;
use arcode_syntax::HighlightSpan;
use arcode_ui_kit::UiTheme;
use gpui::Hsla;

/// Opacity of the blocks: the minimap stays quieter than the code.
pub(crate) const BLOCK_OPACITY: f32 = 0.6;
/// Lines built beyond the view on each side, so short scrolls reuse them.
const MARGIN_LINES: usize = 64;

/// Columns `start..end` of a line drawn in `color`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Segment {
    pub start: u16,
    pub end: u16,
    pub color: Hsla,
}

/// What the blocks were built from.
#[derive(Debug, Clone, PartialEq)]
struct RowsKey {
    version: u64,
    highlights: u64,
    theme: usize,
    theme_name: String,
    tab_size: usize,
    max_cols: usize,
}

/// The blocks of a range of buffer lines.
pub(crate) struct MinimapRows {
    key: RowsKey,
    lines: Range<usize>,
    /// Index of each line's first segment, plus the end.
    starts: Vec<u32>,
    segments: Vec<Segment>,
}

/// Inputs for [`MinimapRows::build`] and [`MinimapRows::is_current`].
pub(crate) struct RowsSource<'a> {
    pub buffer: &'a Buffer,
    /// Buffer version.
    pub version: u64,
    /// Id of the highlight cache (0 without one).
    pub highlights_id: u64,
    /// Lines the highlight cache covers and their spans.
    pub highlights: Option<(Range<usize>, &'a [Vec<HighlightSpan>])>,
    pub theme: &'a UiTheme,
    pub tab_size: usize,
    pub max_cols: usize,
}

impl RowsSource<'_> {
    fn key(&self) -> RowsKey {
        RowsKey {
            version: self.version,
            highlights: self.highlights_id,
            theme: std::ptr::from_ref(self.theme) as usize,
            theme_name: self.theme.name.clone(),
            tab_size: self.tab_size,
            max_cols: self.max_cols,
        }
    }

    fn spans(&self, line: usize) -> &[HighlightSpan] {
        self.highlights
            .as_ref()
            .and_then(|(lines, spans)| spans.get(line.checked_sub(lines.start)?))
            .map_or(&[], Vec::as_slice)
    }
}

impl MinimapRows {
    /// Whether these rows hold `lines` built from `source`.
    pub fn is_current(&self, source: &RowsSource<'_>, lines: &Range<usize>) -> bool {
        let k = &self.key;
        self.lines.start <= lines.start
            && lines.end <= self.lines.end
            && k.version == source.version
            && k.highlights == source.highlights_id
            && k.theme == std::ptr::from_ref(source.theme) as usize
            && k.theme_name == source.theme.name
            && k.tab_size == source.tab_size
            && k.max_cols == source.max_cols
    }

    /// Builds the blocks for `lines` and a margin around them.
    pub fn build(source: &RowsSource<'_>, lines: Range<usize>) -> Self {
        let line_count = source.buffer.line_count();
        let start = lines.start.saturating_sub(MARGIN_LINES).min(line_count);
        let end = (lines.end + MARGIN_LINES).min(line_count).max(start);
        let mut starts = Vec::with_capacity(end - start + 1);
        let mut segments = Vec::new();
        let mut colors = ColorCache::new(source.theme);
        for line in start..end {
            starts.push(segments.len() as u32);
            let text = source.buffer.line(line);
            line_segments(
                text.chunks().flat_map(str::chars),
                source.spans(line),
                source.tab_size,
                source.max_cols,
                &mut colors,
                &mut segments,
            );
        }
        starts.push(segments.len() as u32);
        MinimapRows {
            key: source.key(),
            lines: start..end,
            starts,
            segments,
        }
    }

    /// The blocks of buffer line `line` (empty outside the built range).
    pub fn line(&self, line: usize) -> &[Segment] {
        let Some(ix) = line.checked_sub(self.lines.start) else {
            return &[];
        };
        match (self.starts.get(ix), self.starts.get(ix + 1)) {
            (Some(&a), Some(&b)) => &self.segments[a as usize..b as usize],
            _ => &[],
        }
    }
}

/// Block colours by capture name for one build. Capture names are static
/// strings from the grammar's query, so a pointer comparison finds them.
pub(crate) struct ColorCache<'a> {
    theme: &'a UiTheme,
    known: Vec<(&'static str, Hsla)>,
    default: Hsla,
}

impl<'a> ColorCache<'a> {
    pub fn new(theme: &'a UiTheme) -> Self {
        ColorCache {
            theme,
            known: Vec::new(),
            default: theme.colors.text.opacity(BLOCK_OPACITY),
        }
    }

    fn get(&mut self, name: &'static str) -> Hsla {
        if let Some((_, c)) = self.known.iter().find(|(n, _)| std::ptr::eq(*n, name)) {
            return *c;
        }
        let color = self
            .theme
            .syntax(name)
            .color
            .unwrap_or(self.theme.colors.text)
            .opacity(BLOCK_OPACITY);
        self.known.push((name, color));
        color
    }
}

/// Appends the blocks of one line (`chars`, with highlight `spans` as byte
/// ranges of the line) to `out`. Tabs advance to the next tab stop; blanks
/// end a block unless one blank separates two words of one colour; columns
/// past `max_cols` are dropped.
pub(crate) fn line_segments(
    chars: impl Iterator<Item = char>,
    spans: &[HighlightSpan],
    tab_size: usize,
    max_cols: usize,
    colors: &mut ColorCache<'_>,
    out: &mut Vec<Segment>,
) {
    let tab_size = tab_size.max(1);
    let max_cols = max_cols.min(u16::MAX as usize);
    let mut col = 0usize;
    let mut byte = 0usize;
    let mut span_ix = 0usize;
    // The colour of span `span_ix`, looked up once per span.
    let mut span_color: Option<(usize, Hsla)> = None;
    let mut open: Option<Segment> = None;
    for c in chars {
        if col >= max_cols || c == '\n' || c == '\r' {
            break;
        }
        let width = if c == '\t' {
            tab_size - col % tab_size
        } else {
            1
        };
        if !(c.is_whitespace() || c.is_control()) {
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
            let end = (col + width).min(max_cols) as u16;
            match &mut open {
                // One blank between same-coloured words is bridged: half
                // the quads for comments and keyword runs, and at one pixel
                // per column the gap barely shows.
                Some(seg) if seg.color == color && col <= usize::from(seg.end) + 1 => seg.end = end,
                _ => {
                    out.extend(open.take());
                    open = Some(Segment {
                        start: col as u16,
                        end,
                        color,
                    });
                }
            }
        }
        col += width;
        byte += c.len_utf8();
    }
    out.extend(open);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> UiTheme {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        UiTheme::new(std::sync::Arc::new(theme))
    }

    fn span(range: Range<usize>, name: &'static str) -> HighlightSpan {
        HighlightSpan { range, name }
    }

    fn segments(line: &str, spans: &[HighlightSpan], max_cols: usize) -> Vec<Segment> {
        let theme = theme();
        let mut colors = ColorCache::new(&theme);
        let mut out = Vec::new();
        line_segments(line.chars(), spans, 4, max_cols, &mut colors, &mut out);
        out
    }

    fn cols(segs: &[Segment]) -> Vec<(u16, u16)> {
        segs.iter().map(|s| (s.start, s.end)).collect()
    }

    #[test]
    fn blanks_split_blocks_and_colours_follow_spans() {
        let theme = theme();
        let spans = [span(4..6, "keyword"), span(7..11, "function")];
        let segs = segments("    fn main() {}", &spans, 100);
        // "fn" | "main" | "() {}": "main()" splits by colour, one blank
        // between same-coloured words is bridged.
        assert_eq!(cols(&segs), vec![(4, 6), (7, 11), (11, 16)]);
        let gap = segments("a  b", &[], 100);
        assert_eq!(cols(&gap), vec![(0, 1), (3, 4)], "two blanks are not");
        let keyword = theme
            .syntax("keyword")
            .color
            .unwrap()
            .opacity(BLOCK_OPACITY);
        let function = theme
            .syntax("function")
            .color
            .unwrap()
            .opacity(BLOCK_OPACITY);
        let text = theme.colors.text.opacity(BLOCK_OPACITY);
        assert_eq!(segs[0].color, keyword);
        assert_eq!(segs[1].color, function);
        assert_eq!(segs[2].color, text);
        assert!(segs.iter().all(|s| s.color.a < 1.));
    }

    #[test]
    fn tabs_expand_and_long_lines_are_cut() {
        let segs = segments("\tx\ty", &[], 100);
        assert_eq!(cols(&segs), vec![(4, 5), (8, 9)]);
        let segs = segments(&"a".repeat(500), &[], 80);
        assert_eq!(cols(&segs), vec![(0, 80)]);
        assert!(segments("", &[], 80).is_empty());
        assert!(segments("   \r\n", &[], 80).is_empty());
    }

    #[test]
    fn multibyte_chars_keep_spans_aligned() {
        // "é" is two bytes: the string span starts at byte 4, column 3.
        let spans = [span(4..9, "string")];
        let segs = segments("é = \"ab\"", &spans, 100);
        assert_eq!(cols(&segs), vec![(0, 3), (4, 8)]);
        assert_ne!(segs[1].color, segs[0].color);
    }

    #[test]
    fn rows_are_reused_until_something_changes() {
        let dark = theme();
        let other = theme();
        let text: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        let buffer = Buffer::new(&text);
        let source = |version, highlights_id, tab_size, theme| RowsSource {
            buffer: &buffer,
            version,
            highlights_id,
            highlights: None,
            theme,
            tab_size,
            max_cols: 80,
        };
        let base = source(1, 7, 4, &dark);
        let rows = MinimapRows::build(&base, 100..200);
        assert!(rows.is_current(&base, &(100..200)));
        assert!(
            rows.is_current(&base, &(40..260)),
            "the margin is built too"
        );
        assert!(!rows.is_current(&base, &(0..200)));
        assert_eq!(cols(rows.line(150)), vec![(0, 8)]);
        assert!(rows.line(10).is_empty() && rows.line(5000).is_empty());
        for changed in [
            source(2, 7, 4, &dark),
            source(1, 8, 4, &dark),
            source(1, 7, 8, &dark),
            source(1, 7, 4, &other),
        ] {
            assert!(!rows.is_current(&changed, &(100..200)));
        }
    }
}
