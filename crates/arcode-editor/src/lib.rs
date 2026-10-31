//! The Arcode code editor view.
//!
//! [`Editor`] is a GPUI entity that renders an [`arcode_buffer::Buffer`]
//! with tree-sitter highlighting, selections, multiple cursors and inline
//! diagnostics. Only the visible lines are shaped and painted, and the view
//! redraws only when something changes.
//!
//! Call [`init`] once at startup to register the key bindings and the key
//! routing hook, and make sure the `arcode_ui_kit` theme and settings
//! globals are set before an editor is rendered.
//!
//! # Extension points
//!
//! Other features never patch the renderer; they describe what they need
//! with the plain data types in [`ext`] and hand them to the editor. Every
//! `set_*` call takes a **set key** chosen by the owner (`"git"`,
//! `"debugger"`, ...) and replaces only that owner's items; positions follow
//! edits until the owner replaces them. Colors are theme keys
//! ([`ColorKey`]).
//!
//! | Need | API | Feedback |
//! |------|-----|----------|
//! | Gutter marks in ordered lanes (change bars, run icons, breakpoint dots) | [`Editor::set_gutter_decorations`], [`Editor::set_gutter_lane_visible`] | [`EditorEvent::GutterClicked`] (also for empty breakpoint slots) |
//! | Line / text background (conflicts, current debug line, search matches) | [`Editor::set_background_highlights`] | - |
//! | Virtual text inside a line (inlay hints) or after it (blame) | [`Editor::set_virtual_text`] | - |
//! | Clickable code lenses above or at the end of a line | [`Editor::set_code_lenses`] | [`EditorEvent::CodeLensClicked`] |
//! | Popovers at a buffer position with caller-built content | [`Editor::show_popover`], [`EditorPopover::on_key`] | keys routed to the popover first |
//! | Intercepting keys before the bindings (Vim, completion) | [`Editor::add_key_interceptor`] | return `true` to consume |
//! | Cursor shape (bar / block / underline / hollow) | [`Editor::set_cursor_shape`] | - |
//!
//! Programmatic APIs: [`Editor::scroll_to_line`], [`Editor::set_selections`]
//! / [`Editor::select_ranges`], [`Editor::apply_edits`] (several ranges as
//! one undo step), [`Editor::insert_snippet`] (tab stops, mirrors, Tab /
//! Shift+Tab / Escape), [`Editor::text_in_range`], [`Editor::word_at`],
//! [`Editor::point_to_lsp`] / [`Editor::lsp_to_point`] and
//! [`Editor::position_for_point`] (window position of a buffer point).
//!
//! Virtual text is not part of the buffer: the cursor skips it, it cannot be
//! selected and every reported column is a buffer column.
//!
//! # Hover and definition links
//!
//! A [`HoverProvider`] (set with [`Editor::set_hover_provider`]) supplies
//! documentation for the symbol under the resting mouse or the cursor
//! ([`ShowHover`]) and the definition link underlined while Ctrl (Cmd) is
//! held. Secondary-click dispatches [`GoToDefinition`]; it and the other
//! navigation actions ([`GoToTypeDefinition`], [`GoToImplementation`],
//! [`FindAllReferences`]) are bound in the editor context and handled by
//! the application. See [`hover`]. The hover closes when the window's
//! focus moves elsewhere, so it never covers a picker or overlay.
//!
//! # Code actions, rename, formatting and inlay hints
//!
//! A [`LanguageServices`] provider (set with
//! [`Editor::set_language_services`]) supplies inlay hints (drawn as
//! virtual text, re-asked after edits and on [`Editor::refresh_inlay_hints`]),
//! code actions (a light bulb in the gutter on the cursor line,
//! [`ShowCodeActions`] / [`Refactor`] open a menu grouped by kind), rename
//! ([`RenameSymbol`], an inline field at the symbol) and formatting
//! ([`FormatDocument`], [`FormatSelection`], format on save with a
//! timeout). Right click (or [`ShowContextMenu`]) opens the context menu.
//! See [`lang`].
//!
//! A read-only editor ([`Editor::set_read_only`]) takes no edits, shows no
//! completions and refuses [`Editor::save`].
//!
//! # Vim mode and multiple cursors
//!
//! With `editor.vim_mode` on (or [`ToggleVimMode`]) each editor hands its
//! keys to an [`arcode_vim::Vim`] through a key interceptor; see [`vim`]
//! for the routing, [`Editor::vim_status`] and
//! [`EditorEvent::VimStatusChanged`] for the status bar and
//! [`set_vim_request_handler`] for `:q` and `:e`. Outside Vim:
//! [`SelectAllOccurrences`], [`AddCursorAbove`] / [`AddCursorBelow`],
//! [`UndoCursor`], Alt+click to add or remove a cursor and box selection
//! with Alt+Shift+drag or the middle button.
//!
//! # Large files
//!
//! Above `editor.large_file_bytes` / `editor.large_file_lines` no syntax tree
//! is kept; each viewport is highlighted from a parse of a small excerpt
//! ([`arcode_syntax::highlight_excerpt`]) and bracket matching is textual.
//! See [`Editor::is_large_file`].

mod actions;
mod api;
mod blink;
mod commands;
pub mod completion;
mod decorate;
mod decorate_ext;
mod diagnostics;
mod display;
mod doc_settings;
mod editor;
mod element;
mod event;
pub mod ext;
mod ext_api;
mod gutter;
pub mod hover;
mod input;
pub mod lang;
mod layout;
mod layout_extras;
mod menu;
mod mouse;
mod mouse_column;
mod mouse_ext;
mod paint;
mod render;
mod scroll;
mod snippet;
mod style;
mod syntax_sync;
pub mod vim;

pub use actions::*;
pub use api::ScrollAnchor;
pub use arcode_ui_kit::ColorKey;
pub use completion::{
    CompletionItem, CompletionList, CompletionProvider, CompletionTrigger, Documentation, ItemEdit,
    ParameterInfo, SignatureHelp, SignatureInfo, SignatureRequest, SignatureTriggers,
};
pub use diagnostics::{DiagnosticSeverity, EditorDiagnostic};
pub use doc_settings::IndentOverride;
pub use editor::{Editor, EditorEvent};
pub use ext::{
    BackgroundHighlight, ChangeKind, CodeLens, CursorShape, DotStyle, EditorPopover,
    GutterDecoration, GutterLane, GutterMark, HighlightRange, InterceptorId, KeyInterceptor,
    LensItem, LensPlacement, PopoverPlacement, VirtualText, VirtualTextPosition,
};
pub use hover::{HoverInfo, HoverProvider};
pub use lang::{
    CodeAction, InlayHint, InlayHintKind, InlayHintsToggle, LanguageServices, RenameTarget,
    TextEdits, inlay_hints_enabled,
};
pub use style::EditorStyle;
pub use vim::{
    VimModeToggle, VimRequest, VimRequestHandler, VimStatus, set_vim_request_handler,
    vim_mode_enabled,
};

use gpui::App;

/// Registers the editor's default key bindings (context `"Editor"`).
/// `secondary` means Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    actions::bind_keys(cx);
    ext::keys::init(cx);
    completion::init(cx);
    lang::init(cx);
    cx.on_action(|_: &ToggleVimMode, cx| vim::toggle_vim_mode(cx));
}
