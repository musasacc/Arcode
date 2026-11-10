//! Minimap tiles: the minimap is drawn as images of [`TILE_ROWS`] display
//! rows each, rasterized on the CPU at the window's device pixel size and
//! painted as one sprite per tile. Painting costs a few sprites per frame
//! whatever the file's size. Each tile remembers a hash of what it shows
//! (its rows' text and highlight spans); it is rasterized again only when
//! that changes, or the theme, the width or the scale do. So scrolling
//! reuses the tiles already built, typing rebuilds the tiles of the edited
//! lines, and new highlights rebuild only tiles whose colours change.
//! While neither the buffer version nor the highlight cache changes, a
//! tile is reused without hashing it again.
//!
//! Images that leave the cache are dropped from GPUI's sprite atlas one
//! frame later (the frame on screen may still use them), or when the
//! editor is released.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use arcode_buffer::Buffer;
use arcode_syntax::HighlightSpan;
use arcode_ui_kit::UiTheme;
use gpui::RenderImage;

use super::glyphs::Patterns;
use super::raster::{ColorCache, LineTarget, raster_line};
use crate::ext::{BlockMap, RowKind};

/// Display rows per tile.
pub(crate) const TILE_ROWS: usize = 64;
/// Tiles kept beyond the ones in view, on each side.
const KEEP_AROUND: usize = 2;
/// Marks a block row (or a row past the end) in a tile's row list.
const NO_LINE: u32 = u32::MAX;

/// What every tile was built with.
#[derive(Debug, Clone, PartialEq)]
struct CacheKey {
    /// [`UiTheme::id`]: a new theme value (any change) draws again.
    theme: u64,
    tab_size: usize,
    max_cols: usize,
    cell: (usize, usize),
}

/// Inputs for building tiles.
pub(crate) struct TileSource<'a> {
    pub buffer: &'a Buffer,
    /// Buffer version and highlight cache id: while both stay, so does
    /// every row's content.
    pub stamp: (u64, u64),
    /// Lines the highlight cache covers and their spans.
    pub highlights: Option<(Range<usize>, &'a [Vec<HighlightSpan>])>,
    pub theme: &'a UiTheme,
    pub tab_size: usize,
    /// Columns that fit in the minimap.
    pub max_cols: usize,
    pub patterns: &'a Patterns,
    pub blocks: &'a BlockMap,
    pub line_count: usize,
}

impl TileSource<'_> {
    fn key(&self) -> CacheKey {
        CacheKey {
            theme: self.theme.id(),
            tab_size: self.tab_size,
            max_cols: self.max_cols,
            cell: (self.patterns.col, self.patterns.pitch),
        }
    }

    fn spans(&self, line: usize) -> &[HighlightSpan] {
        self.highlights
            .as_ref()
            .and_then(|(lines, spans)| spans.get(line.checked_sub(lines.start)?))
            .map_or(&[], Vec::as_slice)
    }

    /// The buffer line of each display row of tile `ix`.
    fn tile_lines(&self, ix: usize) -> Vec<u32> {
        let total = self.blocks.total_rows(self.line_count);
        (ix * TILE_ROWS..(ix + 1) * TILE_ROWS)
            .map(|row| match row < total {
                true => match self.blocks.row_kind(row, self.line_count) {
                    RowKind::Text(line) => line as u32,
                    _ => NO_LINE,
                },
                false => NO_LINE,
            })
            .collect()
    }

    /// A hash of what tile rows showing `lines` draw: the chars that fit
    /// and the spans over them.
    fn content_hash(&self, lines: &[u32]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for &line in lines {
            line.hash(&mut h);
            if line == NO_LINE {
                continue;
            }
            let line = line as usize;
            let text = self.buffer.line(line);
            // Every char takes at least one column.
            let text = text.slice(..text.len_chars().min(self.max_cols));
            let mut bytes = 0;
            // Bytes only: the rope's chunk boundaries move with edits.
            for chunk in text.chunks() {
                h.write(chunk.as_bytes());
                bytes += chunk.len();
            }
            bytes.hash(&mut h);
            for span in self
                .spans(line)
                .iter()
                .take_while(|s| s.range.start < bytes)
            {
                (span.range.start, span.range.end).hash(&mut h);
                (span.name.as_ptr() as usize).hash(&mut h);
            }
        }
        h.finish()
    }

    /// The BGRA pixels of a tile showing `lines`, and its size.
    pub fn raster(&self, lines: &[u32]) -> (Vec<u8>, u32, u32) {
        let (cw, pitch) = (self.patterns.col, self.patterns.pitch);
        let width = (self.max_cols * cw).max(1);
        let stride = width * 4;
        let mut pixels = vec![0; stride * lines.len() * pitch];
        let mut colors = ColorCache::new(self.theme);
        for (row, &line) in lines.iter().enumerate() {
            if line == NO_LINE {
                continue;
            }
            let line = line as usize;
            let text = self.buffer.line(line);
            raster_line(
                text.chunks().flat_map(str::chars),
                self.spans(line),
                self.tab_size,
                self.max_cols,
                &mut colors,
                self.patterns,
                &mut LineTarget {
                    pixels: &mut pixels,
                    stride,
                    offset: row * pitch * stride,
                },
            );
        }
        (pixels, width as u32, (lines.len() * pitch) as u32)
    }
}

/// One tile.
struct Tile {
    /// [`TileSource::content_hash`] of what it shows.
    hash: u64,
    /// The stamp and rows it was last checked against.
    stamp: (u64, u64),
    lines: Vec<u32>,
    image: Arc<RenderImage>,
}

/// A tile to paint: its first display row and its image.
#[derive(Clone)]
pub(crate) struct PlacedTile {
    pub first_row: usize,
    pub image: Arc<RenderImage>,
}

/// The tiles of one editor's minimap.
#[derive(Default)]
pub(crate) struct TileCache {
    key: Option<CacheKey>,
    tiles: HashMap<usize, Tile>,
    /// Left the cache this frame; dropped from the atlas next frame.
    retired: Vec<Arc<RenderImage>>,
    /// Rasterized since the cache was created (for tests and logs).
    pub built: usize,
}

impl TileCache {
    /// The tiles covering display `rows`, building those missing or out of
    /// date and retiring those far from view.
    pub fn tiles_for(&mut self, source: &TileSource<'_>, rows: Range<usize>) -> Vec<PlacedTile> {
        let key = source.key();
        if self.key.as_ref() != Some(&key) {
            self.retire_all();
            self.key = Some(key);
        }
        if rows.is_empty() || source.max_cols == 0 {
            return Vec::new();
        }
        let first = rows.start / TILE_ROWS;
        let last = (rows.end - 1) / TILE_ROWS;
        let keep = first.saturating_sub(KEEP_AROUND)..last + KEEP_AROUND + 1;
        let far: Vec<usize> = self
            .tiles
            .keys()
            .copied()
            .filter(|ix| !keep.contains(ix))
            .collect();
        for ix in far {
            self.retire(ix);
        }
        (first..=last)
            .map(|ix| {
                let lines = source.tile_lines(ix);
                let mut hash = None;
                if let Some(tile) = self.tiles.get_mut(&ix)
                    && (tile.stamp != source.stamp || tile.lines != lines)
                {
                    let h = *hash.insert(source.content_hash(&lines));
                    if tile.hash == h {
                        tile.stamp = source.stamp;
                        tile.lines.clone_from(&lines);
                    } else {
                        self.retire(ix);
                    }
                }
                let tile = self.tiles.entry(ix).or_insert_with(|| {
                    self.built += 1;
                    let (pixels, w, h) = source.raster(&lines);
                    Tile {
                        image: Arc::new(to_image(pixels, w, h)),
                        hash: hash.unwrap_or_else(|| source.content_hash(&lines)),
                        stamp: source.stamp,
                        lines,
                    }
                });
                PlacedTile {
                    first_row: ix * TILE_ROWS,
                    image: tile.image.clone(),
                }
            })
            .collect()
    }

    fn retire(&mut self, ix: usize) {
        if let Some(tile) = self.tiles.remove(&ix) {
            self.retired.push(tile.image);
        }
    }

    /// Retires every tile (the minimap was hidden or everything changed).
    pub fn retire_all(&mut self) {
        let ixs: Vec<usize> = self.tiles.keys().copied().collect();
        for ix in ixs {
            self.retire(ix);
        }
    }

    /// Images retired since the last call.
    pub fn take_retired(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.retired)
    }

    /// Every image the cache holds or retired, emptying it.
    pub fn take_all(&mut self) -> Vec<Arc<RenderImage>> {
        self.retire_all();
        self.key = None;
        self.take_retired()
    }

    /// Tiles held.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
}

/// A GPUI image of BGRA `pixels`, `w` x `h`.
fn to_image(pixels: Vec<u8>, w: u32, h: u32) -> RenderImage {
    let buffer = image::RgbaImage::from_raw(w, h, pixels).expect("buffer matches its size");
    RenderImage::new(vec![image::Frame::new(buffer)])
}

#[cfg(test)]
#[path = "tiles_tests.rs"]
mod tests;
