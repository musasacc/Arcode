//! Tile building, reuse and retirement.

use super::*;

fn theme() -> UiTheme {
    let registry = arcode_config::ThemeRegistry::builtin();
    let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
    UiTheme::new(std::sync::Arc::new(theme))
}

struct Fixture {
    buffer: Buffer,
    theme: UiTheme,
    other: UiTheme,
    patterns: Patterns,
    blocks: BlockMap,
    spans: Vec<Vec<HighlightSpan>>,
    /// Stands for the highlight cache id.
    highlights_id: u64,
}

impl Fixture {
    fn new(lines: usize) -> Self {
        let text: String = (0..lines).map(|i| format!("line {i}\n")).collect();
        Fixture {
            buffer: Buffer::new(&text),
            theme: theme(),
            other: theme(),
            patterns: Patterns::new(1, 2),
            blocks: BlockMap::default(),
            spans: Vec::new(),
            highlights_id: 0,
        }
    }

    fn source(&self) -> TileSource<'_> {
        TileSource {
            buffer: &self.buffer,
            stamp: (self.buffer.version(), self.highlights_id),
            highlights: Some((0..self.spans.len(), self.spans.as_slice())),
            theme: &self.theme,
            tab_size: 4,
            max_cols: 80,
            patterns: &self.patterns,
            blocks: &self.blocks,
            line_count: self.buffer.line_count(),
        }
    }
}

#[test]
fn tiles_cover_the_rows_and_are_reused() {
    let f = Fixture::new(1000);
    let mut cache = TileCache::default();
    let tiles = cache.tiles_for(&f.source(), 100..500);
    let firsts: Vec<usize> = tiles.iter().map(|t| t.first_row).collect();
    assert_eq!(firsts, vec![64, 128, 192, 256, 320, 384, 448]);
    assert_eq!(cache.built, 7);
    let size = tiles[0].image.size(0);
    assert_eq!((size.width.0, size.height.0), (80, 128));
    // Scrolling a little reuses them and builds the new one only.
    cache.tiles_for(&f.source(), 140..540);
    assert_eq!(cache.built, 8);
    assert!(cache.take_retired().is_empty());
    // Scrolling far retires the tiles left behind.
    cache.tiles_for(&f.source(), 900..1000);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.take_retired().len(), 8);
}

#[test]
fn only_tiles_whose_content_changes_are_rebuilt() {
    let mut f = Fixture::new(1000);
    let mut cache = TileCache::default();
    cache.tiles_for(&f.source(), 0..192);
    assert_eq!(cache.built, 3);
    // Typing on line 70 changes tile 1 only.
    let at = f.buffer.line_start(70);
    f.buffer.edit(vec![(at..at, "x".into())]);
    cache.tiles_for(&f.source(), 0..192);
    assert_eq!(cache.built, 4);
    // New highlights: tiles whose spans are unchanged stay.
    f.spans = vec![Vec::new(); 300];
    f.highlights_id = 1;
    cache.tiles_for(&f.source(), 0..192);
    assert_eq!(cache.built, 4, "no spans before or after");
    f.spans[130] = vec![HighlightSpan {
        range: 0..4,
        name: "keyword",
    }];
    f.highlights_id = 2;
    cache.tiles_for(&f.source(), 0..192);
    assert_eq!(cache.built, 5, "tile 2 only");
    // A new line shifts every line below it.
    let at = f.buffer.line_start(100);
    f.buffer.edit(vec![(at..at, "\n".into())]);
    cache.tiles_for(&f.source(), 0..192);
    assert_eq!(cache.built, 7);
    // Another theme repaints everything.
    let mut other = f.source();
    other.theme = &f.other;
    cache.tiles_for(&other, 0..192);
    assert_eq!(cache.built, 10);
    assert_eq!(cache.take_retired().len(), 7);
    // A lens row moves the lines of the tiles below it.
    f.blocks = BlockMap::new(vec![150], 1000);
    let mut other = f.source();
    other.theme = &f.other;
    cache.tiles_for(&other, 0..192);
    assert_eq!(cache.built, 11, "only the tile with the lens row");
    assert_eq!(cache.take_all().len(), 1 + 3);
    assert_eq!(cache.len(), 0);
}

// A theme edited live is a new value of the same name, which can sit at
// the address of the one the tiles were built with: they are built again.
#[test]
fn a_new_theme_value_in_the_same_place_repaints() {
    let mut f = Fixture::new(200);
    let mut cache = TileCache::default();
    cache.tiles_for(&f.source(), 0..128);
    assert_eq!(cache.built, 2);
    let before = std::ptr::from_ref(&f.theme);
    f.theme = theme();
    assert_eq!(
        std::ptr::from_ref(&f.theme),
        before,
        "same address and name"
    );
    cache.tiles_for(&f.source(), 0..128);
    assert_eq!(cache.built, 4);
}

#[test]
fn block_rows_and_rows_past_the_end_stay_empty() {
    let mut f = Fixture::new(10);
    f.blocks = BlockMap::new(vec![2], 10);
    let lines = f.source().tile_lines(0);
    assert_eq!(&lines[..4], &[0, 1, NO_LINE, 2]);
    // The buffer ends with a newline: line 10 is the empty last line.
    assert_eq!(&lines[10..12], &[9, 10]);
    assert!(lines[12..].iter().all(|&l| l == NO_LINE));
    let (pixels, w, h) = f.source().raster(&lines);
    assert_eq!((w, h), (80, 128));
    let lit = |row: usize| {
        let stride = w as usize * 4;
        pixels[row * stride..(row + 1) * stride]
            .chunks(4)
            .any(|p| p[3] > 0)
    };
    // Pixel row 3 is the lower half of row 1 (line 1), 5 of the lens row.
    assert!(lit(3));
    assert!(!lit(5));
    assert!(lit(7));
    assert!(!lit(23));
}
