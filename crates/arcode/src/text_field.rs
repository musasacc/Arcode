//! A minimal single-line text input (used by the file finder).
//!
//! Supports typing (including IME through [`EntityInputHandler`]), cursor
//! movement, selection with shift, select-all and clipboard. Offsets are
//! UTF-8 byte indices on char boundaries.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, KeyBinding, Pixels, ShapedLine, SharedString, TextRun,
    UTF16Selection, UnderlineStyle, Window, actions, canvas, div, fill, point, prelude::*, px,
    size,
};

use arcode_ui_kit::theme;

actions!(
    text_field,
    [Backspace, Delete, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Copy, Cut]
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
        }
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
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
        let new: String = new.chars().filter(|c| *c != '\n' && *c != '\r').collect();
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
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx);
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

    fn to_utf16(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())].encode_utf16().count()
    }

    fn from_utf16(&self, offset: usize) -> usize {
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
        self.from_utf16(r.start)..self.from_utf16(r.end)
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.to_utf16(r.start)..self.to_utf16(r.end)
    }

    fn paint_text(this: Entity<Self>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let c = theme(cx).colors.clone();
        let field = this.read(cx);
        let focus = field.focus_handle.clone();
        let focused = focus.is_focused(window);
        let style = window.text_style();
        let (text, color) = if field.text.is_empty() {
            (field.placeholder.clone(), c.text_disabled)
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
        let line = window.text_system().shape_line(text, font_size, &runs, None);
        let line_height = bounds.size.height;
        if !selection.is_empty() {
            let x0 = line.x_for_index(selection.start);
            let x1 = line.x_for_index(selection.end);
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left() + x0, bounds.top()),
                    size(x1 - x0, line_height),
                ),
                c.selection,
            ));
        }
        let text_top = bounds.top() + (line_height - window.line_height()) / 2.;
        line.paint(point(bounds.left(), text_top), window.line_height(), window, cx)
            .ok();
        if focused {
            let x = line.x_for_index(cursor);
            window.paint_quad(fill(
                Bounds::new(point(bounds.left() + x, text_top), size(px(1.5), window.line_height())),
                c.cursor,
            ));
        }
        window.handle_input(&focus, ElementInputHandler::new(bounds, this.clone()), cx);
        this.update(cx, |field, _| field.layout = Some((line, bounds)));
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range);
        actual.replace(self.range_to_utf16(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selection),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.range_from_utf16(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.replace(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.range_from_utf16(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.replace(range.clone(), text, cx);
        let len = self.text.len().min(range.start + text.len());
        self.marked = (!text.is_empty()).then(|| range.start..len);
        if let Some(sel) = selected {
            let a = (range.start + sel.start).min(self.text.len());
            let b = (range.start + sel.end).min(self.text.len());
            self.selection = a..b;
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (line, bounds) = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (line, bounds) = self.layout.as_ref()?;
        let index = line.index_for_x(p.x - bounds.left())?;
        Some(self.to_utf16(index))
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .key_context(CONTEXT)
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
            .child(
                canvas(|_, _, _| {}, move |bounds, _, window, cx| {
                    Self::paint_text(this.clone(), bounds, window, cx)
                })
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn editing_and_utf16_mapping(cx: &mut TestAppContext) {
        let field = cx.new(|cx| TextField::new("", cx));
        field.update(cx, |f, cx| {
            f.replace(0..0, "aé😀b", cx);
            assert_eq!(f.text(), "aé😀b");
            assert_eq!(f.to_utf16(f.text.len()), 5);
            assert_eq!(f.from_utf16(3), "aé".len());
            let end = f.text.len();
            assert_eq!(f.prev_boundary(end - 1), "aé".len());
            f.selection = 1..1;
            f.replace(0..1, "", cx);
            assert_eq!(f.text(), "é😀b");
        });
    }
}
