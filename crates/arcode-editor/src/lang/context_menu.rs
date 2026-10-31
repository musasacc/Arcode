//! The editor's context menu (right click, Shift+F10): navigation, rename,
//! refactor, format and the clipboard. Entries dispatch the same actions
//! as their key bindings, which are shown next to them.

use std::rc::Rc;

use arcode_buffer::{Selection, SelectionSet};
use gpui::{Action, Context, Keystroke, MouseDownEvent, Window};

use super::CONTEXT_MENU_POPOVER;
use crate::Editor;
use crate::layout::LayoutSnapshot;
use crate::menu::{MenuItem, MenuRow, OnPick};

/// A context menu entry (`None` is a separator): label, action, enabled.
type Entry = (&'static str, Box<dyn Action>, bool);

/// `Ctrl+Shift+F`-style label of a keystroke for the current platform.
pub(crate) fn keystroke_label(ks: &Keystroke) -> String {
    let m = &ks.modifiers;
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<String> = Vec::new();
    if m.control {
        parts.push("Ctrl".into());
    }
    if m.alt {
        parts.push(if mac { "Option" } else { "Alt" }.into());
    }
    if m.shift {
        parts.push("Shift".into());
    }
    if m.platform {
        parts.push(if mac { "Cmd" } else { "Super" }.into());
    }
    let mut key = ks.key.chars();
    let key = key
        .next()
        .map(|first| first.to_uppercase().chain(key).collect::<String>())
        .unwrap_or_default();
    parts.push(key);
    parts.join("+")
}

/// The key binding shown for `action`: the first one using the secondary
/// modifier (Ctrl+X rather than Shift+Delete or Ctrl+Insert) when there is
/// one, else the one that takes precedence.
fn binding_label(
    window: &Window,
    action: &dyn Action,
    focus: &gpui::FocusHandle,
) -> Option<String> {
    let bindings = window.bindings_for_action_in(action, focus);
    let secondary = |b: &&gpui::KeyBinding| {
        b.keystrokes().first().is_some_and(|k| {
            let m = k.inner().modifiers;
            if cfg!(target_os = "macos") {
                m.platform
            } else {
                m.control
            }
        })
    };
    let binding = bindings
        .iter()
        .find(secondary)
        .or_else(|| bindings.last())?;
    Some(
        binding
            .keystrokes()
            .iter()
            .map(|k| keystroke_label(k.inner()))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

impl Editor {
    /// A right click: moves the cursor there unless it is inside a
    /// selection, then opens the context menu.
    pub(crate) fn context_click(
        &mut self,
        event: &MouseDownEvent,
        layout: &LayoutSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = event.position;
        if p.x < layout.text_bounds.left() {
            self.gutter_context_click(event, layout, cx);
            return;
        }
        let pos = self.char_for_position(layout, p);
        let inside = self.selections.iter().any(|s| {
            let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
            a < b && a <= pos && pos <= b
        });
        if !inside {
            self.selections = SelectionSet::from_selection(Selection::cursor(pos));
            self.selections_changed(cx);
        }
        let at = self.buffer.char_to_point(pos);
        self.show_context_menu_at(at, window, cx);
    }

    /// Opens the context menu at the cursor (Shift+F10).
    pub fn show_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.cursor_position();
        self.show_context_menu_at(at, window, cx);
    }

    fn show_context_menu_at(
        &mut self,
        at: arcode_buffer::Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editable = !self.read_only;
        let has_selection = self.selections.iter().any(|s| s.anchor != s.head);
        let mut entries: Vec<Option<Entry>> = vec![
            Some(("Go to Definition", Box::new(crate::GoToDefinition), true)),
            Some((
                "Find All References",
                Box::new(crate::FindAllReferences),
                true,
            )),
            None,
            Some(("Rename Symbol", Box::new(crate::RenameSymbol), editable)),
            Some(("Refactor...", Box::new(crate::Refactor), editable)),
            Some(("Format Document", Box::new(crate::FormatDocument), editable)),
        ];
        if has_selection {
            entries.push(Some((
                "Format Selection",
                Box::new(crate::FormatSelection),
                editable,
            )));
        }
        entries.extend([
            None,
            Some(("Cut", Box::new(crate::Cut) as Box<dyn Action>, editable)),
            Some(("Copy", Box::new(crate::Copy), true)),
            Some(("Paste", Box::new(crate::Paste), editable)),
        ]);
        let mut rows = Vec::new();
        let mut actions: Vec<Option<Box<dyn Action>>> = Vec::new();
        for entry in entries {
            match entry {
                None => {
                    rows.push(MenuRow::Separator);
                    actions.push(None);
                }
                Some((label, action, enabled)) => {
                    let binding = binding_label(window, action.as_ref(), &self.focus_handle);
                    rows.push(MenuRow::Item(
                        MenuItem::new(label).keys(binding).enabled(enabled),
                    ));
                    actions.push(Some(action));
                }
            }
        }
        let on_pick: OnPick = Rc::new(move |_, row, window, cx| {
            if let Some(Some(action)) = actions.get(row) {
                window.dispatch_action(action.boxed_clone(), cx);
            }
        });
        self.show_menu(CONTEXT_MENU_POPOVER, at, rows, on_pick, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_read_like_menus() {
        let ks = Keystroke::parse("shift-alt-f").unwrap();
        let alt = if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        };
        assert_eq!(keystroke_label(&ks), format!("{alt}+Shift+F"));
        assert_eq!(keystroke_label(&Keystroke::parse("f12").unwrap()), "F12");
        assert_eq!(
            keystroke_label(&Keystroke::parse("ctrl-.").unwrap()),
            "Ctrl+."
        );
    }
}
