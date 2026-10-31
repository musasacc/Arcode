//! The inline rename field: a one-line text input drawn in a popover at
//! the symbol. The editor keeps focus; its popover key handler feeds the
//! keys to [`RenameField::key`].

use std::ops::Range;

use arcode_ui_kit::{popover, settings, theme, type_scale, ui_font_family};
use gpui::{
    Context, IntoElement, Keystroke, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*, px,
};

/// What a keystroke did to the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldKey {
    /// The text or the selection changed.
    Edited,
    /// Swallowed without effect.
    Consumed,
    /// Enter.
    Submit,
    /// Escape.
    Cancel,
    /// Copy this text to the clipboard.
    Copy(String),
    /// Not for the field (e.g. Ctrl+S): the editor gets it.
    Ignored,
}

/// Text, cursor and selection of the field (char indices).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenameField {
    text: String,
    cursor: usize,
    anchor: usize,
}

impl RenameField {
    /// A field holding `text`, all of it selected.
    pub fn new(text: &str) -> Self {
        let len = text.chars().count();
        RenameField {
            text: text.to_string(),
            cursor: len,
            anchor: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The selected char range.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    fn byte(&self, char_ix: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_ix)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Replaces the selection with `s` (line breaks dropped).
    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        let sel = self.selection();
        let (a, b) = (self.byte(sel.start), self.byte(sel.end));
        self.text.replace_range(a..b, &s);
        self.cursor = sel.start + s.chars().count();
        self.anchor = self.cursor;
    }

    fn delete(&mut self, forward: bool) {
        if self.anchor == self.cursor {
            if forward && self.cursor < self.len() {
                self.cursor += 1;
            } else if !forward && self.cursor > 0 {
                self.anchor -= 1;
            }
        }
        self.insert("");
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.cursor = to.min(self.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Applies a keystroke. `clipboard` is read only for paste.
    pub fn key(&mut self, ks: &Keystroke, clipboard: impl FnOnce() -> Option<String>) -> FieldKey {
        let m = &ks.modifiers;
        let secondary = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        let sel = self.selection();
        match ks.key.as_str() {
            "enter" => return FieldKey::Submit,
            "escape" => return FieldKey::Cancel,
            "left" if sel.is_empty() || m.shift => {
                self.move_to(self.cursor.saturating_sub(1), m.shift)
            }
            "left" => self.move_to(sel.start, false),
            "right" if sel.is_empty() || m.shift => self.move_to(self.cursor + 1, m.shift),
            "right" => self.move_to(sel.end, false),
            "home" => self.move_to(0, m.shift),
            "end" => self.move_to(self.len(), m.shift),
            "backspace" => self.delete(false),
            "delete" => self.delete(true),
            "a" if secondary => {
                self.anchor = 0;
                self.cursor = self.len();
            }
            "v" if secondary => match clipboard() {
                Some(text) => self.insert(text.lines().next().unwrap_or_default()),
                None => return FieldKey::Consumed,
            },
            "c" if secondary => {
                let (a, b) = (self.byte(sel.start), self.byte(sel.end));
                return FieldKey::Copy(self.text[a..b].to_string());
            }
            // Option composes characters on macOS; elsewhere Alt is a
            // shortcut modifier.
            _ if m.control || m.platform || (m.alt && !cfg!(target_os = "macos")) => {
                return FieldKey::Ignored;
            }
            key => {
                // Test platforms leave `key_char` empty for plain keys.
                let typed = ks.key_char.clone().or_else(|| match key {
                    "space" => Some(" ".to_string()),
                    k if k.chars().count() == 1 => Some(if m.shift {
                        k.to_uppercase()
                    } else {
                        k.to_string()
                    }),
                    _ => None,
                });
                match typed {
                    Some(text) if !text.chars().any(char::is_control) => self.insert(&text),
                    _ => return FieldKey::Consumed,
                }
            }
        }
        FieldKey::Edited
    }
}

/// The field's view.
pub(crate) struct RenameView {
    pub field: RenameField,
}

impl Render for RenameView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        let config = settings(cx);
        let ui_size = type_scale(cx).meta;
        let code_size = px(config.editor.font_size);
        let family = SharedString::from(config.editor.font_family.clone());
        let text = &self.field.text;
        let sel = self.field.selection();
        let (a, b) = (self.field.byte(sel.start), self.field.byte(sel.end));
        let caret = || div().w(px(1.5)).h(code_size * 1.3).bg(colors.cursor);
        let caret_first = self.field.cursor == sel.start && !sel.is_empty();
        let segment = |s: &str| {
            div()
                .whitespace_nowrap()
                .child(SharedString::from(s.to_string()))
        };
        let line = div()
            .flex()
            .flex_row()
            .items_center()
            .child(segment(&text[..a]))
            .when(caret_first, |d| d.child(caret()))
            .child(segment(&text[a..b]).bg(colors.selection))
            .when(!caret_first, |d| d.child(caret()))
            .child(segment(&text[b..]));
        // A popover holding a focused input (docs/DESIGN.md 6.12, 6.13).
        popover(cx)
            .id("rename-field")
            .occlude()
            .gap(px(4.))
            .font_family(ui_font_family(cx))
            .child(
                div()
                    .min_w(px(220.))
                    .px(px(8.))
                    .py(px(3.))
                    .bg(colors.canvas)
                    .border_1()
                    .border_color(colors.accent)
                    .rounded(px(7.))
                    .font_family(family)
                    .text_size(code_size)
                    .text_color(colors.text)
                    .child(line),
            )
            .child(
                div()
                    .px(px(4.))
                    .pb(px(2.))
                    .text_size(ui_size)
                    .text_color(colors.text_muted)
                    .child("Enter to rename, Escape to cancel"),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(key: &str) -> Keystroke {
        Keystroke::parse(key).unwrap()
    }

    fn press(field: &mut RenameField, key: &str) -> FieldKey {
        field.key(&ks(key), || Some("pasted\nmore".into()))
    }

    #[test]
    fn typing_replaces_the_selected_name() {
        let mut f = RenameField::new("count");
        assert_eq!(f.selection(), 0..5);
        press(&mut f, "t");
        press(&mut f, "o");
        press(&mut f, "shift-t");
        press(&mut f, "space");
        assert_eq!(f.text(), "toT ");
        press(&mut f, "backspace");
        press(&mut f, "left");
        press(&mut f, "backspace");
        assert_eq!(f.text(), "tT");
        press(&mut f, "home");
        press(&mut f, "delete");
        assert_eq!(f.text(), "T");
        assert_eq!(press(&mut f, "enter"), FieldKey::Submit);
        assert_eq!(press(&mut f, "escape"), FieldKey::Cancel);
    }

    #[test]
    fn selection_paste_copy_and_shortcuts() {
        let mut f = RenameField::new("größe");
        press(&mut f, "right");
        press(&mut f, "shift-left");
        press(&mut f, "shift-left");
        assert_eq!(f.selection(), 3..5);
        let secondary = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        assert_eq!(
            press(&mut f, &format!("{secondary}-c")),
            FieldKey::Copy("ße".into())
        );
        press(&mut f, &format!("{secondary}-v"));
        assert_eq!(f.text(), "gröpasted");
        press(&mut f, &format!("{secondary}-a"));
        assert_eq!(f.selection(), 0..9);
        assert_eq!(press(&mut f, &format!("{secondary}-s")), FieldKey::Ignored);
        assert_eq!(press(&mut f, "f5"), FieldKey::Consumed);
        assert_eq!(press(&mut f, "tab"), FieldKey::Consumed);
        assert_eq!(f.text(), "gröpasted");
    }
}
