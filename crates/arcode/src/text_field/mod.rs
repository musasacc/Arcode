//! A minimal text input: single-line (file finder, prompts) or, with
//! [`TextField::multiline`], a small multi-line box (commit messages).
//!
//! Supports typing (including IME through `EntityInputHandler`), cursor
//! movement, selection with shift, select-all and clipboard. Offsets are
//! UTF-8 byte indices on char boundaries.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, ElementInputHandler, Entity, EventEmitter,
    FocusHandle, Focusable, KeyBinding, Pixels, ShapedLine, SharedString, TextRun, UnderlineStyle,
    Window, actions, canvas, div, fill, point, prelude::*, px, size,
};

use arcode_ui_kit::theme;

actions!(
    text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Copy,
        Cut
    ]
);

/// Key context of the text field.
pub const CONTEXT: &str = "TextField";

/// Registers the text field key bindings.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("secondary-a", SelectAll, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("secondary-v", Paste, ctx),
        KeyBinding::new("secondary-c", Copy, ctx),
        KeyBinding::new("secondary-x", Cut, ctx),
    ]);
}

/// Emitted when the text changes.
pub struct TextChanged;

/// A single-line text input.
pub struct TextField {
    focus_handle: FocusHandle,
    text: String,
    placeholder: SharedString,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    layout: Option<(ShapedLine, Bounds<Pixels>)>,
    /// Byte offset where the line in `layout` starts (multi-line mode).
    layout_start: usize,
    /// Multi-line mode state; `None` for a single-line field.
    multi: Option<multiline::MultiState>,
}

impl EventEmitter<TextChanged> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextField {
    /// Creates an empty field showing `placeholder`.
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            text: String::new(),
            placeholder: placeholder.into(),
            selection: 0..0,
            reversed: false,
            marked: None,
            layout: None,
            layout_start: 0,
            multi: None,
        }
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and puts the cursor at the end.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let len = self.text.len();
        self.replace(0..len, text, cx);
    }

    /// Selects the whole text (typing then replaces it).
    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.selection = 0..self.text.len();
        self.reversed = false;
        cx.notify();
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    fn prev_boundary(&self, offset: usize) -> usize {
        self.text[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.text[offset..]
            .chars()
            .next()
            .map_or(self.text.len(), |c| offset + c.len_utf8())
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selection = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        cx.notify();
    }

    fn replace(&mut self, range: Range<usize>, new: &str, cx: &mut Context<Self>) {
        let multi = self.multi.is_some();
        let new: String = new
            .chars()
            .filter(|c| *c != '\r' && (multi || *c != '\n'))
            .collect();
        self.text.replace_range(range.clone(), &new);
        let end = range.start + new.len();
        self.selection = end..end;
        self.reversed = false;
        self.marked = None;
        cx.emit(TextChanged);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            let start = self.prev_boundary(self.cursor());
            self.selection = start..self.selection.end;
        }
        self.replace(self.selection.clone(), "", cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            let end = self.next_boundary(self.cursor());
            self.selection = self.selection.start..end;
        }
        self.replace(self.selection.clone(), "", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selection.is_empty() {
            self.prev_boundary(self.cursor())
        } else {
            self.selection.start
        };
        self.move_to(to, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selection.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selection.end
        };
        self.move_to(to, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection = 0..self.text.len();
        self.reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_bounds(self.cursor()).start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_bounds(self.cursor()).end, cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace(self.selection.clone(), &text, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.text[self.selection.clone()].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        self.replace(self.selection.clone(), "", cx);
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())]
            .encode_utf16()
            .count()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut units = 0;
        for (i, c) in self.text.char_indices() {
            if units >= offset {
                return i;
            }
            units += c.len_utf16();
        }
        self.text.len()
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn paint_text(this: Entity<Self>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        if this.read(cx).multi.is_some() {
            Self::paint_lines(this, bounds, window, cx);
            return;
        }
        let c = theme(cx).colors.clone();
        let field = this.read(cx);
        let focus = field.focus_handle.clone();
        let focused = focus.is_focused(window);
        let style = window.text_style();
        let (text, color) = if field.text.is_empty() {
            // A hint people read: muted, not `text_disabled` (that one is
            // for disabled controls only, docs/DESIGN.md 2.4).
            (field.placeholder.clone(), c.text_muted)
        } else {
            (SharedString::from(field.text.clone()), c.text)
        };
        let marked = field.marked.clone();
        let (selection, cursor) = if field.text.is_empty() {
            (0..0, 0)
        } else {
            (field.selection.clone(), field.cursor())
        };
        let mut runs = vec![TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        if let Some(marked) = marked.filter(|m| m.end <= text.len()) {
            let base = runs.remove(0);
            let underline = Some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy: false,
            });
            runs = [
                (marked.start, None),
                (marked.len(), underline),
                (text.len() - marked.end, None),
            ]
            .into_iter()
            .filter(|(len, _)| *len > 0)
            .map(|(len, underline)| TextRun {
                len,
                underline,
                ..base.clone()
            })
            .collect();
        }
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);
        let line_height = bounds.size.height;
        // Keep the caret in view: text wider than the field scrolls left
        // while it is focused.
        let shift = if focused {
            (line.x_for_index(cursor) + px(2.) - bounds.size.width).max(px(0.))
        } else {
            px(0.)
        };
        let left = bounds.left() - shift;
        let text_top = bounds.top() + (line_height - window.line_height()) / 2.;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if !selection.is_empty() {
                let x0 = line.x_for_index(selection.start);
                let x1 = line.x_for_index(selection.end);
                window.paint_quad(fill(
                    Bounds::new(point(left + x0, bounds.top()), size(x1 - x0, line_height)),
                    c.selection,
                ));
            }
            line.paint(point(left, text_top), window.line_height(), window, cx)
                .ok();
            if focused {
                let x = line.x_for_index(cursor);
                window.paint_quad(fill(
                    Bounds::new(
                        point(left + x, text_top),
                        size(px(1.5), window.line_height()),
                    ),
                    c.cursor,
                ));
            }
        });
        let bounds_shifted = Bounds::new(point(left, bounds.top()), bounds.size);
        window.handle_input(&focus, ElementInputHandler::new(bounds, this.clone()), cx);
        this.update(cx, |field, _| {
            field.layout = Some((line, bounds_shifted));
            field.layout_start = 0;
        });
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let multi = self.multi.is_some();
        div()
            .key_context(if multi { MULTILINE_CONTEXT } else { CONTEXT })
            .track_focus(&self.focus_handle)
            .cursor_text()
            .flex_1()
            .h_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .when(multi, |el| self.multiline_actions(el, cx))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| Self::paint_text(this.clone(), bounds, window, cx),
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests;

mod ime;
mod multiline;

pub use multiline::{MULTILINE_CONTEXT, bind_multiline_keys};
