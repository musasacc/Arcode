//! GPUI view for Arcode's integrated terminal.
//!
//! [`TerminalView`] owns an [`arcode_terminal::Terminal`], draws its grid
//! with a custom element (redrawing only on terminal wakeups or input),
//! resizes the PTY to fit, and forwards keyboard, mouse, paste and IME
//! input. Call [`init`] once at startup to register the key bindings.
//!
//! The view reads the `arcode_ui_kit` settings and theme globals, so they
//! (and the bundled fonts) must be set up before a view is created.

mod colors;
mod element;
mod ime;
mod input;
mod layout;
mod mouse;
mod view;

use gpui::{App, KeyBinding};

pub use colors::{palette_from_theme, rgb_to_hsla};
pub use view::{TerminalView, TerminalViewEvent};

/// Terminal actions, bound in the `"Terminal"` key context.
pub mod actions {
    gpui::actions!(
        terminal,
        [
            /// Copies the selection to the clipboard.
            Copy,
            /// Pastes the clipboard (bracketed when the program asks for it).
            Paste,
            /// Clears the screen.
            Clear,
            /// Scrolls one page up into the history.
            ScrollPageUp,
            /// Scrolls one page down towards the live output.
            ScrollPageDown,
        ]
    );
}

/// The key context set on the terminal view.
pub const KEY_CONTEXT: &str = "Terminal";

/// Registers the terminal key bindings (context `"Terminal"`): copy and
/// paste (Cmd+C/V on macOS, Ctrl+Shift+C/V elsewhere), clear (Cmd+K on
/// macOS) and page scrolling (Shift+PageUp/PageDown).
pub fn init(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("shift-pageup", actions::ScrollPageUp, ctx),
        KeyBinding::new("shift-pagedown", actions::ScrollPageDown, ctx),
    ];
    if cfg!(target_os = "macos") {
        bindings.extend([
            KeyBinding::new("cmd-c", actions::Copy, ctx),
            KeyBinding::new("cmd-v", actions::Paste, ctx),
            KeyBinding::new("cmd-k", actions::Clear, ctx),
        ]);
    } else {
        bindings.extend([
            KeyBinding::new("ctrl-shift-c", actions::Copy, ctx),
            KeyBinding::new("ctrl-shift-v", actions::Paste, ctx),
        ]);
    }
    cx.bind_keys(bindings);
}
