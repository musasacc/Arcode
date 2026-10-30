//! Per-frame layout: metrics, scroll resolution and shaping of the visible
//! lines only. The result is kept for hit testing until the next frame.
//!
//! Display rows are buffer lines plus block rows (code lenses drawn above a
//! line); [`BlockMap`] converts between the two.

use std::ops::Range;
use std::rc::Rc;

use arcode_buffer::movement;
use arcode_ui_kit::{apply_glyph_fallbacks, theme};
use gpui::{App, Bounds, Context, Pixels, ShapedLine, SharedString, Window, point, px, size};

use crate::Editor;
use crate::api::ScrollAnchor;
use crate::display::{DisplayLine, InlayText, MAX_DISPLAY_CHARS, build_display_line, text_run};
use crate::editor::Autoscroll;
use crate::ext::{BlockMap, RowKind};
use crate::gutter::GutterGeometry;
use crate::layout_extras::{ExtraItem, ExtraRow, LensHit};
use crate::scroll::{scrollbar_layout, vertical_autoscroll};
use crate::style::EditorStyle;

/// Width of the vertical scrollbar.
pub(crate) const SCROLLBAR_WIDTH: Pixels = px(12.);
/// Width of the diagnostic marker column.
pub(crate) const MARKER_WIDTH: Pixels = px(14.);

/// One shaped visible line.
pub(crate) struct VisibleLine {
    pub line: usize,
    pub display: DisplayLine,
    pub shaped: ShapedLine,
    /// End-of-line virtual text and lenses, x relative to the text origin.
    pub eol: Vec<ExtraItem>,
}

/// Scrollbar geometry in window coordinates.
#[derive(Clone, Copy)]
pub(crate) struct ScrollbarLayout {
    pub track: Bounds<Pixels>,
    pub thumb: Bounds<Pixels>,
}

/// Everything painting and hit testing need from one layout pass.
pub(crate) struct LayoutSnapshot {
    pub bounds: Bounds<Pixels>,
    pub text_bounds: Bounds<Pixels>,
    pub gutter_width: Pixels,
    pub gutter: GutterGeometry,
    pub line_height: Pixels,
    pub char_width: Pixels,
    pub scroll_x: Pixels,
    pub scroll_y: Pixels,
    pub max_scroll_y: Pixels,
    pub lines: Vec<VisibleLine>,
    pub blocks: BlockMap,
    /// Visible block rows (lenses above lines).
    pub block_rows: Vec<ExtraRow>,
    /// Clickable lens items in window coordinates.
    pub lens_hits: Vec<LensHit>,
    pub tab_size: usize,
    pub scrollbar: Option<ScrollbarLayout>,
    pub style: EditorStyle,
}

impl LayoutSnapshot {
    /// Window y of the top of buffer line `line`'s text row.
    pub fn line_top(&self, line: usize) -> Pixels {
        self.text_bounds.top() + self.line_height * self.blocks.row_for_line(line) as f32
            - self.scroll_y
    }

    /// Window x of the text origin (column 0, after horizontal scroll).
    pub fn text_left(&self) -> Pixels {
        self.text_bounds.left() - self.scroll_x
    }

    /// The shaped line for buffer line `line`, if visible.
    pub fn visible(&self, line: usize) -> Option<&VisibleLine> {
        let first = self.lines.first()?.line;
        self.lines.get(line.checked_sub(first)?)
    }

    /// Buffer lines in view.
    pub fn visible_range(&self) -> Range<usize> {
        match (self.lines.first(), self.lines.last()) {
            (Some(a), Some(b)) => a.line..b.line + 1,
            _ => 0..0,
        }
    }

    /// x offset (from the text origin) of the caret at char column `col`.
    pub fn x_for(&self, line: usize, col: usize) -> Pixels {
        match self.visible(line) {
            Some(v) => v.shaped.x_for_index(v.display.byte_for_col(col)),
            None => self.char_width * col as f32,
        }
    }

    /// x offset where the glyph of char column `col` starts (after any
    /// inlay text at that column).
    pub fn x_for_glyph(&self, line: usize, col: usize) -> Pixels {
        match self.visible(line) {
            Some(v) => v.shaped.x_for_index(v.display.char_start(col)),
            None => self.char_width * col as f32,
        }
    }

    /// What is on the display row at window y `y`.
    pub fn row_at(&self, y: Pixels, line_count: usize) -> RowKind {
        let rel = y - self.text_bounds.top() + self.scroll_y;
        let row = if rel < Pixels::ZERO {
            0
        } else {
            (rel / self.line_height).floor() as usize
        };
        self.blocks.row_kind(row, line_count)
    }

    /// Visible lines per page.
    pub fn page_lines(&self) -> usize {
        ((self.text_bounds.size.height / self.line_height).floor() as usize).max(1)
    }
}

impl Editor {
    /// Lays out the editor for `bounds`, resolving pending autoscroll.
    pub(crate) fn layout(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Rc<LayoutSnapshot> {
        let started = std::time::Instant::now();
        let style = self.style(cx);
        let lh = style.line_height;
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&style.font);
        let char_width = text_system
            .advance(font_id, style.font_size, 'm')
            .map(|s| s.width)
            .unwrap_or(style.font_size * 0.6);
        let line_count = self.buffer.line_count();
        let digits = line_count.to_string().len().max(2);
        let gutter = GutterGeometry::new(
            &self.ext.active_lanes(),
            style.show_line_numbers,
            char_width,
            digits,
        );
        let gutter_width = gutter.width;
        let text_bounds = Bounds::new(
            point(bounds.left() + gutter_width, bounds.top()),
            size(
                (bounds.size.width - gutter_width - SCROLLBAR_WIDTH).max(px(1.)),
                bounds.size.height,
            ),
        );
        let blocks = BlockMap::new(self.ext.block_lines(), line_count);
        let viewport_h = text_bounds.size.height;
        let content_h = lh * blocks.total_rows(line_count) as f32;
        let max_scroll_y = if content_h <= viewport_h {
            Pixels::ZERO
        } else {
            (content_h - viewport_h * 0.5).max(Pixels::ZERO)
        };

        let head = self.selections.primary().head;
        let cursor_line = self.buffer.char_to_line(head);
        let autoscroll = self.autoscroll.take();
        if let Some(mode) = autoscroll {
            let (line, mode) = match mode {
                Autoscroll::Line(line, anchor) => (line, anchor),
                Autoscroll::Center => (cursor_line, ScrollAnchor::Center),
                Autoscroll::Fit => (cursor_line, ScrollAnchor::Fit),
            };
            // Keep a lens row above the line in view too.
            let row = blocks.row_for_line(line) - usize::from(blocks.has_block(line));
            let rows = 1 + usize::from(blocks.has_block(line));
            self.scroll_y = vertical_autoscroll(mode, self.scroll_y, row, rows, lh, viewport_h);
        }
        self.scroll_y = self.scroll_y.clamp(Pixels::ZERO, max_scroll_y);

        let first_row = (self.scroll_y / lh).floor() as usize;
        let last_row = first_row + (viewport_h / lh).ceil() as usize + 1;
        let first = blocks.row_kind(first_row, line_count).line();
        let last = (blocks.row_kind(last_row, line_count).line() + 1).min(line_count);
        self.ensure_highlights(first..last, cx);
        let lines = self.shape_lines(first..last, &style, window, cx);

        let mut max_width = lines
            .iter()
            .map(|l| l.shaped.width)
            .fold(Pixels::ZERO, Pixels::max);
        let cursor_autoscroll = matches!(autoscroll, Some(Autoscroll::Fit | Autoscroll::Center));
        if cursor_autoscroll {
            let col = head - self.buffer.line_start(cursor_line);
            let cursor_x = lines
                .iter()
                .find(|l| l.line == cursor_line)
                .map(|l| l.shaped.x_for_index(l.display.byte_for_col(col)))
                .unwrap_or(char_width * col as f32);
            let margin = char_width * 4.;
            let text_w = text_bounds.size.width;
            if cursor_x < self.scroll_x + margin {
                self.scroll_x = (cursor_x - margin).max(Pixels::ZERO);
            } else if cursor_x + margin > self.scroll_x + text_w {
                self.scroll_x = cursor_x + margin - text_w;
            }
            max_width = max_width.max(cursor_x);
        }
        let max_scroll_x = (max_width + char_width * 4. - text_bounds.size.width).max(Pixels::ZERO);
        self.scroll_x = self.scroll_x.clamp(Pixels::ZERO, max_scroll_x);

        let scrollbar = scrollbar_layout(bounds, self.scroll_y, max_scroll_y, viewport_h);
        let mut snapshot = LayoutSnapshot {
            bounds,
            text_bounds,
            gutter_width,
            gutter,
            line_height: lh,
            char_width,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            max_scroll_y,
            lines,
            blocks,
            block_rows: Vec::new(),
            lens_hits: Vec::new(),
            tab_size: style.tab_size,
            scrollbar,
            style,
        };
        self.layout_extras(&mut snapshot, window, cx);
        let snapshot = Rc::new(snapshot);
        self.layout = Some(snapshot.clone());
        log::trace!(
            "layout of {} lines took {:?}",
            snapshot.lines.len(),
            started.elapsed()
        );
        snapshot
    }

    /// Builds display strings and shapes buffer lines `range`.
    fn shape_lines(
        &mut self,
        range: Range<usize>,
        style: &EditorStyle,
        window: &mut Window,
        cx: &App,
    ) -> Vec<VisibleLine> {
        let theme = theme(cx);
        let colors = &theme.colors;
        let spans = self.highlights.as_ref();
        let fallbacks: Vec<SharedString> = style
            .font
            .fallbacks
            .as_ref()
            .map(|f| f.fallback_list().iter().map(|s| s.clone().into()).collect())
            .unwrap_or_default();
        let mut text = String::new();
        range
            .map(|line| {
                text.clear();
                let slice = self.buffer.line(line);
                let slice = slice.slice(..slice.len_chars().min(MAX_DISPLAY_CHARS + 1));
                for chunk in slice.chunks() {
                    text.push_str(chunk);
                }
                let trimmed = text.trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}', '\u{85}']);
                let line_spans = spans
                    .and_then(|c| c.spans.get(line.checked_sub(c.lines.start)?))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let inline = self.ext.inline_text(line);
                let inlays: Vec<InlayText<'_>> = inline
                    .iter()
                    .map(|(col, v)| InlayText {
                        col: *col,
                        text: v.text.as_ref(),
                        color: v.color.resolve(colors),
                        background: v.background.as_ref().map(|b| b.resolve(colors)),
                        italic: v.italic,
                    })
                    .collect();
                let mut display = build_display_line(
                    trimmed,
                    line_spans,
                    &inlays,
                    style.tab_size,
                    &style.font,
                    theme,
                );
                if let Some(runs) = apply_glyph_fallbacks(
                    &display.text,
                    &display.runs,
                    &fallbacks,
                    window.text_system(),
                ) {
                    display.runs = runs;
                }
                let shaped = window.text_system().shape_line(
                    display.text.clone(),
                    style.font_size,
                    &display.runs,
                    None,
                );
                VisibleLine {
                    line,
                    display,
                    shaped,
                    eol: Vec::new(),
                }
            })
            .collect()
    }

    /// Buffer char index for a window position (clamped into the buffer).
    /// A position on a block row maps to the line below it.
    pub(crate) fn char_for_position(
        &self,
        layout: &LayoutSnapshot,
        p: gpui::Point<Pixels>,
    ) -> usize {
        let line = layout.row_at(p.y, self.buffer.line_count()).line();
        let x = (p.x - layout.text_left()).max(Pixels::ZERO);
        let col = match layout.visible(line) {
            Some(v) => v.display.col_for_byte(v.shaped.closest_index_for_x(x)),
            None => {
                let display_col = (x / layout.char_width).round() as usize;
                return movement::char_for_display_column(
                    &self.buffer,
                    line,
                    display_col,
                    layout.tab_size,
                );
            }
        };
        let col = col.min(self.buffer.line_len(line));
        self.buffer.line_start(line) + col
    }

    /// Shapes a gutter or popover label with the editor font.
    pub(crate) fn shape_label(
        text: SharedString,
        style: &EditorStyle,
        color: gpui::Hsla,
        window: &mut Window,
    ) -> ShapedLine {
        let run = text_run(text.len(), &style.font, &Default::default(), color);
        window
            .text_system()
            .shape_line(text, style.font_size, &[run], None)
    }
}
