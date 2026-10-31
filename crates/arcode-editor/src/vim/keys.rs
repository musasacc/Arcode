//! From GPUI keystrokes to Vim keys, and the key interceptor.

use arcode_vim::{Key, Mode, VimAction};
use gpui::{App, Entity, Keystroke, Window};

use super::{RequestHandler, VimRequest};
use crate::Editor;

/// The Vim key for `ks`, or `None` when the keystroke always belongs to
/// the application: modifier-only keys, Cmd (macOS) and Ctrl+Shift
/// combinations, function keys, and Ctrl+P (the file finder; Vim's Ctrl-P
/// is the same as `k`).
pub fn to_vim_key(ks: &Keystroke) -> Option<Key> {
    let m = &ks.modifiers;
    let name = ks.key.as_str();
    let modifier_only = matches!(
        name,
        "shift" | "control" | "alt" | "platform" | "function" | "capslock"
    );
    let function_key =
        name.len() > 1 && name.starts_with('f') && name[1..].chars().all(|c| c.is_ascii_digit());
    if modifier_only || function_key || m.platform || m.function || (m.control && m.shift) {
        return None;
    }
    if m.control && !m.alt && name == "p" {
        return None;
    }
    let plain = !m.control && !m.alt;
    let text = ks
        .key_char
        .as_deref()
        .filter(|t| plain && !t.is_empty() && !t.chars().any(char::is_control));
    let key = match text {
        // `key_char` already carries Shift (and the keyboard layout).
        Some(t) if name != "space" => Key::new(t),
        _ => Key {
            key: name.to_string(),
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
        },
    };
    Some(key)
}

/// The interceptor installed in Vim mode. Runs Vim on the keystroke and,
/// outside the editor update, any request for the application.
pub(super) fn intercept(
    editor: &Entity<Editor>,
    ks: &Keystroke,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    // The second key of a chord (Ctrl+K Ctrl+I) finishes the chord.
    if window.has_pending_keystrokes() {
        return false;
    }
    let (consumed, request) = editor.update(cx, |e, cx| e.vim_keystroke(ks, cx));
    if let Some(request) = request {
        let handler = cx.try_global::<RequestHandler>().map(|h| h.0.clone());
        match handler {
            Some(handler) => handler(editor, request, window, cx),
            None => log::warn!("vim: no handler for {request:?}"),
        }
    }
    consumed
}

impl Editor {
    /// Handles `ks` in Vim mode; returns whether it was consumed and a
    /// request for the application.
    fn vim_keystroke(
        &mut self,
        ks: &Keystroke,
        cx: &mut gpui::Context<Self>,
    ) -> (bool, Option<VimRequest>) {
        let Some(mode) = self.vim_mode() else {
            return (false, None);
        };
        let Some(key) = to_vim_key(ks) else {
            return (false, None);
        };
        let inserting = matches!(mode, Mode::Insert | Mode::Replace);
        let tab = key.key == "tab" && !key.ctrl && !key.alt;
        if inserting && tab && self.snippet_active() {
            // Tab jumps to the next snippet placeholder.
            return (false, None);
        }
        let idle_normal =
            mode == Mode::Normal && self.vim_status().is_some_and(|s| s.pending.is_empty());
        if idle_normal && key.key == "escape" && self.selections.len() > 1 {
            // Escape in Normal mode goes back to one cursor.
            self.update_selections(cx, |_, s| s.collapse_to_primary());
            return (true, None);
        }
        let (action, request) = self.vim_feed(&key, cx);
        if action != VimAction::PassThrough {
            return (true, request);
        }
        // Vim does not use the key: the editor's binding runs, except
        // that plain keys never type text outside Insert mode.
        let m = &ks.modifiers;
        let combo = m.control || m.alt;
        (!inserting && !combo, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> Option<Key> {
        to_vim_key(&Keystroke::parse(source).unwrap())
    }

    #[test]
    fn plain_and_shifted_keys() {
        assert_eq!(key("a"), Some(Key::new("a")));
        let shifted = key("shift-a").unwrap();
        assert_eq!((shifted.key.as_str(), shifted.shift), ("a", true));
        assert_eq!(key("$"), Some(Key::new("$")));
        assert_eq!(key("escape"), Some(Key::new("escape")));
        let mut with_char = Keystroke::parse("shift-4").unwrap();
        with_char.key_char = Some("$".into());
        assert_eq!(to_vim_key(&with_char), Some(Key::new("$")));
        let mut space = Keystroke::parse("space").unwrap();
        space.key_char = Some(" ".into());
        assert_eq!(to_vim_key(&space), Some(Key::new("space")));
    }

    #[test]
    fn control_keys() {
        assert_eq!(key("ctrl-d"), Some(Key::ctrl("d")));
        let ctrl_v = key("ctrl-v").unwrap();
        assert!(ctrl_v.ctrl);
        assert_eq!(key("ctrl-p"), None, "the finder keeps Ctrl+P");
        assert_eq!(key("ctrl-shift-p"), None, "the palette keeps Ctrl+Shift+P");
        assert_eq!(key("cmd-s"), None);
        assert_eq!(key("f12"), None);
        assert_eq!(key("shift"), None);
        let alt = key("alt-x").unwrap();
        assert!(alt.alt);
    }
}
