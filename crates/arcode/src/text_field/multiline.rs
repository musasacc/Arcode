//! Multi-line mode of [`TextField`]: Enter inserts a line break, Up/Down
//! move between lines and the box grows from `min_rows` to `max_rows`
//! lines, scrolling to keep the cursor visible beyond that.

use std::ops::Range;

use arcode_ui_kit::theme;
use gpui::{
    App, Bounds, Context, ElementInputHandler, Entity, KeyBinding, Pixels, SharedString, TextRun,
    Window, actions, fill, point, prelude::*, px, size,
};

use super::TextField;

actions!(text_field, [Newline, Up, Down]);

/// Key context of a multi-line field (it also matches `TextField`).
pub const MULTILINE_CONTEXT: &str = "TextField multiline";

/// Registers Enter / Up / Down for multi-line fields.
pub fn bind_multiline_keys(cx: &mut App) {
    let ctx = Some("TextField && multiline");
    cx.bind_keys([
        KeyBinding::new("enter", Newline, ctx),
        KeyBinding::new("shift-enter", Newline, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
    ]);
}

/// Layout state of a multi-line field.
#[derive(Debug, Clone, Default)]
pub(super) struct MultiState {
    min_rows: usize,
    max_rows: usize,
    /// First visible line.
    scroll: usize,
}

impl TextField {
    /// A multi-line field showing between `min_rows` and `max_rows` lines.
    pub fn multiline(
        placeholder: impl Into<SharedString>,
        min_rows: usize,
        max_rows: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = Self::new(placeholder, cx);
        field.multi = Some(MultiState {
            min_rows: min_rows.max(1),
            max_rows: max_rows.max(min_rows.max(1)),
            scroll: 0,
        });
        field
    }

    /// Number of lines the field shows (1 for a single-line field).
    pub fn visible_rows(&self) -> usize {
        match &self.multi {
            Some(m) => self.text.split('\n').count().clamp(m.min_rows, m.max_rows),
            None => 1,
        }
    }

    /// Byte range of the line containing `offset` (without its `\n`).
    pub(super) fn line_bounds(&self, offset: usize) -> Range<usize> {
        if self.multi.is_none() {
            return 0..self.text.len();
        }
        let offset = offset.min(self.text.len());
        let start = self.text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i);
        start..end
    }

    /// Offset on the line above (or below) at the same char column.
    fn vertical(&self, down: bool) -> usize {
        let cursor = self.cursor();
        let line = self.line_bounds(cursor);
        let column = self.text[line.start..cursor].chars().count();
        let target = if down {
            if line.end >= self.text.len() {
                return self.text.len();
            }
            self.line_bounds(line.end + 1)
        } else {
            if line.start == 0 {
                return 0;
            }
            self.line_bounds(line.start - 1)
        };
        let text = &self.text[target.clone()];
        target.start
            + text
                .char_indices()
                .nth(column)
                .map_or(text.len(), |(i, _)| i)
    }

    pub(super) fn multiline_actions<E: InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|f: &mut Self, _: &Newline, _, cx| {
                f.replace(f.selection.clone(), "\n", cx)
            }),
        )
        .on_action(cx.listener(|f: &mut Self, _: &Up, _, cx| {
            let to = f.vertical(false);
            f.move_to(to, cx);
        }))
        .on_action(cx.listener(|f: &mut Self, _: &Down, _, cx| {
            let to = f.vertical(true);
            f.move_to(to, cx);
        }))
    }

    /// Paints a multi-line field: each visible line, the selection, the
    /// cursor, and registers the input handler.
    pub(super) fn paint_lines(
        this: Entity<Self>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let c = theme(cx).colors.clone();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let field = this.read(cx);
        let focus = field.focus_handle.clone();
        let focused = focus.is_focused(window);
        let empty = field.text.is_empty();
        let rows = field.visible_rows();
        let cursor = field.cursor();
        let selection = field.selection.clone();
        let text = if empty {
            field.placeholder.to_string()
        } else {
            field.text.clone()
        };
        let color = if empty { c.text_muted } else { c.text };
        let cursor_line = field.text[..cursor.min(field.text.len())]
            .matches('\n')
            .count();
        let mut scroll = field.multi.as_ref().map_or(0, |m| m.scroll);
        if cursor_line < scroll {
            scroll = cursor_line;
        } else if cursor_line >= scroll + rows {
            scroll = cursor_line + 1 - rows;
        }
        let mut start = 0;
        let mut cursor_layout = None;
        for (ix, line) in text.split('\n').enumerate() {
            let range = start..start + line.len();
            start = range.end + 1;
            if ix < scroll || ix >= scroll + rows {
                continue;
            }
            let top = bounds.top() + line_height * (ix - scroll) as f32;
            let run = TextRun {
                len: line.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(
                SharedString::from(line.to_owned()),
                font_size,
                &[run],
                None,
            );
            if !empty && selection.start <= range.end && selection.end >= range.start {
                let a = selection.start.max(range.start) - range.start;
                let b = selection.end.min(range.end) - range.start;
                let tail = if selection.end > range.end {
                    px(6.)
                } else {
                    px(0.)
                };
                let x0 = shaped.x_for_index(a);
                let x1 = shaped.x_for_index(b) + tail;
                if x1 > x0 {
                    window.paint_quad(fill(
                        Bounds::new(point(bounds.left() + x0, top), size(x1 - x0, line_height)),
                        c.selection,
                    ));
                }
            }
            shaped
                .paint(point(bounds.left(), top), line_height, window, cx)
                .ok();
            let line_bounds = Bounds::new(
                point(bounds.left(), top),
                size(bounds.size.width, line_height),
            );
            if ix == cursor_line || (empty && ix == 0) {
                if focused {
                    let x = if empty {
                        px(0.)
                    } else {
                        shaped.x_for_index(cursor - range.start)
                    };
                    window.paint_quad(fill(
                        Bounds::new(point(bounds.left() + x, top), size(px(1.5), line_height)),
                        c.cursor,
                    ));
                }
                cursor_layout = Some((shaped, line_bounds, if empty { 0 } else { range.start }));
            }
        }
        window.handle_input(&focus, ElementInputHandler::new(bounds, this.clone()), cx);
        this.update(cx, |field, _| {
            if let Some(multi) = field.multi.as_mut() {
                multi.scroll = scroll;
            }
            if let Some((line, bounds, start)) = cursor_layout {
                field.layout = Some((line, bounds));
                field.layout_start = start;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn multiline_keeps_breaks_and_moves_vertically(cx: &mut TestAppContext) {
        let field = cx.new(|cx| TextField::multiline("", 2, 4, cx));
        field.update(cx, |f, cx| {
            f.replace(0..0, "first\r\nsecond line\nx", cx);
            assert_eq!(f.text(), "first\nsecond line\nx");
            assert_eq!(f.visible_rows(), 3);
            f.selection = 3..3;
            assert_eq!(f.vertical(true), "first\nsec".len());
            f.selection = 15..15;
            assert_eq!(
                f.vertical(true),
                f.text().len(),
                "clamped to the short line"
            );
            assert_eq!(f.vertical(false), 5, "end of `first`");
            assert_eq!(f.line_bounds(8), 6..17);
        });
        let single = cx.new(|cx| TextField::new("", cx));
        single.update(cx, |f, cx| {
            f.replace(0..0, "a\nb", cx);
            assert_eq!(f.text(), "ab");
            assert_eq!(f.visible_rows(), 1);
        });
    }
}
