//! Per-frame layout: metrics, scroll resolution and shaping of the visible
//! lines only. The result is kept for hit testing until the next frame.

use std::ops::Range;
use std::rc::Rc;

use arcode_buffer::movement;
use arcode_ui_kit::theme;
use gpui::{App, Bounds, Context, Pixels, ShapedLine, SharedString, Window, point, px, size};

use crate::display::{DisplayLine, MAX_DISPLAY_CHARS, build_display_line, text_run};
use crate::editor::{Autoscroll, HighlightCache};
use crate::style::EditorStyle;
use crate::Editor;

/// Width of the vertical scrollbar.
pub(crate) const SCROLLBAR_WIDTH: Pixels = px(12.);
/// Width of the diagnostic marker column at the gutter's left edge.
pub(crate) const MARKER_WIDTH: Pixels = px(14.);
/// Extra lines highlighted around the viewport so small scrolls reuse them.
const HIGHLIGHT_MARGIN: usize = 32;

/// One shaped visible line.
pub(crate) struct VisibleLine {
    pub line: usize,
    pub display: DisplayLine,
    pub shaped: ShapedLine,
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
    pub line_height: Pixels,
    pub char_width: Pixels,
    pub scroll_x: Pixels,
    pub scroll_y: Pixels,
    pub max_scroll_y: Pixels,
    pub lines: Vec<VisibleLine>,
    pub tab_size: usize,
    pub scrollbar: Option<ScrollbarLayout>,
    pub style: EditorStyle,
}

impl LayoutSnapshot {
    /// Window y of the top of buffer line `line`.
    pub fn line_top(&self, line: usize) -> Pixels {
        self.text_bounds.top() + self.line_height * line as f32 - self.scroll_y
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

    /// x offset (from the text origin) of char column `col` on `line`.
    pub fn x_for(&self, line: usize, col: usize) -> Pixels {
        match self.visible(line) {
            Some(v) => v.shaped.x_for_index(v.display.byte_for_col(col)),
            None => self.char_width * col as f32,
        }
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
        let gutter_width = if style.show_line_numbers {
            MARKER_WIDTH + char_width * digits as f32 + px(18.)
        } else {
            MARKER_WIDTH + px(6.)
        };
        let text_bounds = Bounds::new(
            point(bounds.left() + gutter_width, bounds.top()),
            size(
                (bounds.size.width - gutter_width - SCROLLBAR_WIDTH).max(px(1.)),
                bounds.size.height,
            ),
        );
        let viewport_h = text_bounds.size.height;
        let content_h = lh * line_count as f32;
        let max_scroll_y = (content_h - viewport_h * 0.5).max(Pixels::ZERO);
        let max_scroll_y = if content_h <= viewport_h {
            Pixels::ZERO
        } else {
            max_scroll_y
        };

        let head = self.selections.primary().head;
        let cursor_line = self.buffer.char_to_line(head);
        let autoscroll = self.autoscroll.take();
        if let Some(mode) = autoscroll {
            self.scroll_y = vertical_autoscroll(mode, self.scroll_y, cursor_line, lh, viewport_h);
        }
        self.scroll_y = self.scroll_y.clamp(Pixels::ZERO, max_scroll_y);

        let first = ((self.scroll_y / lh).floor() as usize).min(line_count.saturating_sub(1));
        let last = (first + (viewport_h / lh).ceil() as usize + 1).min(line_count);
        let lines = self.shape_lines(first..last, &style, window, cx);

        let mut max_width = lines
            .iter()
            .map(|l| l.shaped.width)
            .fold(Pixels::ZERO, Pixels::max);
        if autoscroll.is_some() {
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
        let max_scroll_x =
            (max_width + char_width * 4. - text_bounds.size.width).max(Pixels::ZERO);
        self.scroll_x = self.scroll_x.clamp(Pixels::ZERO, max_scroll_x);

        let scrollbar = scrollbar_layout(bounds, self.scroll_y, max_scroll_y, viewport_h);
        let snapshot = Rc::new(LayoutSnapshot {
            bounds,
            text_bounds,
            gutter_width,
            line_height: lh,
            char_width,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            max_scroll_y,
            lines,
            tab_size: style.tab_size,
            scrollbar,
            style,
        });
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
        self.ensure_highlights(range.clone());
        let theme = theme(cx);
        let spans = self.highlights.as_ref();
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
                    .and_then(|c| c.spans.get(line - c.lines.start))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let display =
                    build_display_line(trimmed, line_spans, style.tab_size, &style.font, theme);
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
                }
            })
            .collect()
    }

    /// Makes sure the highlight cache covers `range` at the current version.
    fn ensure_highlights(&mut self, range: Range<usize>) {
        let Some(syntax) = self.syntax.as_ref() else {
            self.highlights = None;
            return;
        };
        let version = self.buffer.version();
        if let Some(cache) = &self.highlights
            && cache.version == version
            && cache.lines.start <= range.start
            && range.end <= cache.lines.end
        {
            return;
        }
        let start = range.start.saturating_sub(HIGHLIGHT_MARGIN);
        let end = (range.end + HIGHLIGHT_MARGIN).min(self.buffer.line_count());
        let spans = syntax.highlight_lines(self.buffer.text(), start..end);
        self.highlights = Some(HighlightCache {
            version,
            lines: start..end,
            spans,
        });
    }

    /// Buffer char index for a window position (clamped into the buffer).
    pub(crate) fn char_for_position(&self, layout: &LayoutSnapshot, p: gpui::Point<Pixels>) -> usize {
        let rel_y = p.y - layout.text_bounds.top() + layout.scroll_y;
        let last = self.buffer.line_count().saturating_sub(1);
        let line = if rel_y < Pixels::ZERO {
            0
        } else {
            ((rel_y / layout.line_height).floor() as usize).min(last)
        };
        let x = (p.x - layout.text_left()).max(Pixels::ZERO);
        let col = match layout.visible(line) {
            Some(v) => v.display.col_for_byte(v.shaped.closest_index_for_x(x)),
            None => {
                let display_col = (x / layout.char_width).round() as usize;
                let pos = movement::char_for_display_column(
                    &self.buffer,
                    line,
                    display_col,
                    layout.tab_size,
                );
                return pos;
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

fn vertical_autoscroll(
    mode: Autoscroll,
    scroll_y: Pixels,
    line: usize,
    lh: Pixels,
    viewport_h: Pixels,
) -> Pixels {
    let top = lh * line as f32;
    let bottom = top + lh;
    let visible = top >= scroll_y && bottom <= scroll_y + viewport_h;
    match mode {
        Autoscroll::Center if !visible => (top - (viewport_h - lh) * 0.5).max(Pixels::ZERO),
        Autoscroll::Center => scroll_y,
        Autoscroll::Fit => {
            let margin = (lh * 2.).min((viewport_h - lh).max(Pixels::ZERO) * 0.5);
            if top - margin < scroll_y {
                (top - margin).max(Pixels::ZERO)
            } else if bottom + margin > scroll_y + viewport_h {
                bottom + margin - viewport_h
            } else {
                scroll_y
            }
        }
    }
}

fn scrollbar_layout(
    bounds: Bounds<Pixels>,
    scroll_y: Pixels,
    max_scroll_y: Pixels,
    viewport_h: Pixels,
) -> Option<ScrollbarLayout> {
    if max_scroll_y <= Pixels::ZERO {
        return None;
    }
    let track = Bounds::new(
        point(bounds.right() - SCROLLBAR_WIDTH, bounds.top()),
        size(SCROLLBAR_WIDTH, bounds.size.height),
    );
    let content = max_scroll_y + viewport_h;
    let thumb_h = (track.size.height * (viewport_h / content)).max(px(24.)).min(track.size.height);
    let frac = scroll_y / max_scroll_y;
    let thumb_y = track.top() + (track.size.height - thumb_h) * frac.clamp(0., 1.);
    Some(ScrollbarLayout {
        track,
        thumb: Bounds::new(point(track.left() + px(2.), thumb_y), size(SCROLLBAR_WIDTH - px(4.), thumb_h)),
    })
}
