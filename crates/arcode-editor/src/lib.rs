//! The Arcode code editor view.
//!
//! [`Editor`] is a GPUI entity that renders an [`arcode_buffer::Buffer`]
//! with tree-sitter highlighting, selections, multiple cursors and inline
//! diagnostics. Only the visible lines are shaped and painted, and the view
//! redraws only when something changes.
//!
//! Call [`init`] once at startup to register the key bindings, and make sure
//! the `arcode_ui_kit` theme and settings globals are set before an editor
//! is rendered.

mod actions;
mod blink;
mod commands;
mod decorate;
mod diagnostics;
mod display;
mod editor;
mod element;
mod input;
mod layout;
mod mouse;
mod paint;
mod render;
mod style;

pub use actions::*;
pub use diagnostics::{DiagnosticSeverity, EditorDiagnostic};
pub use editor::{Editor, EditorEvent};
pub use style::EditorStyle;

use gpui::App;

/// Registers the editor's default key bindings (context `"Editor"`).
/// `secondary` means Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    actions::bind_keys(cx);
}
