//! Optional Vim mode, built on [`arcode_vim`] and the key-interceptor
//! extension point.
//!
//! When `editor.vim_mode` is on (or the session toggle
//! [`VimModeToggle`] says so), every editor installs a key interceptor that
//! hands keystrokes to its own [`Vim`] state machine. Popover handlers run
//! first, so a completion list, the rename field or a code-action menu
//! keep their keys (Escape closes the completion list before it leaves
//! Insert mode). Registers, macros and the last search are shared by all
//! editors ([`VimShared`], a GPUI global), so a yank in one file pastes in
//! another; `"+` and `"*` are the system clipboard.
//!
//! The cursor is a block in Normal and Visual modes, a bar in Insert mode
//! and an underline in Replace and operator-pending mode. The mode, the
//! keys of an unfinished command, the command line being typed (`:` / `/`)
//! and Vim's messages are published through [`Editor::vim_status`] and
//! [`EditorEvent::VimStatusChanged`](crate::EditorEvent::VimStatusChanged)
//! for the status bar.
//!
//! Ex commands the editor cannot run alone (`:q`, `:q!`, `:wq`, `:x`,
//! `:e <file>`) go to the application's [`VimRequestHandler`] (see
//! [`set_vim_request_handler`]).

mod keys;
mod run;
mod search;
mod status;

use std::rc::Rc;

use arcode_vim::{Mode, Vim, VimShared};
use gpui::{App, Context, Entity, Global, Window};

use crate::Editor;
use crate::ext::{CursorShape, InterceptorId};

pub use keys::to_vim_key;
pub use status::status_element;

/// Session-wide override of `editor.vim_mode` ("Editor: Toggle Vim Mode").
#[derive(Debug, Default, Clone, Copy)]
pub struct VimModeToggle(pub Option<bool>);

impl Global for VimModeToggle {}

/// Whether Vim mode is on: the session toggle, else `editor.vim_mode` (or
/// `editor.keymap = "vim"`).
pub fn vim_mode_enabled(cx: &App) -> bool {
    if let Some(on) = cx.try_global::<VimModeToggle>().and_then(|t| t.0) {
        return on;
    }
    cx.try_global::<arcode_ui_kit::Settings>().is_some_and(|_| {
        let editor = &arcode_ui_kit::settings(cx).editor;
        editor.vim_mode || editor.keymap == arcode_config::Keymap::Vim
    })
}

/// Flips the session-wide Vim mode toggle ("Editor: Toggle Vim Mode").
/// Every editor follows on its next render; all windows are refreshed.
/// Installed as an app-wide handler of [`ToggleVimMode`](crate::ToggleVimMode)
/// by [`init`](crate::init), so it works whatever has focus.
pub fn toggle_vim_mode(cx: &mut App) {
    let enabled = !vim_mode_enabled(cx);
    cx.set_global(VimModeToggle(Some(enabled)));
    cx.refresh_windows();
}

/// Registers, macros and the last search of every editor.
#[derive(Default)]
struct SharedVim(VimShared);

impl Global for SharedVim {}

/// What Vim shows in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimStatus {
    /// The mode label: `NORMAL`, `INSERT`, `VISUAL`, `V-LINE`, `V-BLOCK`,
    /// `REPLACE`, `O-PENDING` or `COMMAND`.
    pub mode: &'static str,
    /// The mode itself.
    pub kind: Mode,
    /// Keys of an unfinished command (`2d`, `"a`), Vim's "showcmd".
    pub pending: String,
    /// The command line being typed, with its prompt (`:%s/a/b`, `/foo`).
    pub command_line: Option<String>,
    /// The last message (`Pattern not found: x`, `3 substitutions`).
    pub message: Option<String>,
    /// The register a macro is being recorded into.
    pub recording: Option<char>,
}

/// An ex command only the application can carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimRequest {
    /// `:q` (`force: false`, only sent when the buffer is saved) or `:q!`
    /// (`force: true`, discard changes): close this editor's tab. `:wq`
    /// and `:x` save first and then send `Close { force: false }`.
    Close {
        /// Close without asking, discarding unsaved changes.
        force: bool,
    },
    /// `:e <file>`: open the file as typed (relative to the project root
    /// or absolute).
    Open(String),
}

/// Handles [`VimRequest`]s for the application.
pub type VimRequestHandler = Rc<dyn Fn(&Entity<Editor>, VimRequest, &mut Window, &mut App)>;

struct RequestHandler(VimRequestHandler);

impl Global for RequestHandler {}

/// Installs the handler for `:q`, `:e` and friends (once, at startup).
pub fn set_vim_request_handler(handler: VimRequestHandler, cx: &mut App) {
    cx.set_global(RequestHandler(handler));
}

/// Vim state of one editor.
pub(crate) struct VimState {
    vim: Vim,
    interceptor: InterceptorId,
    /// Inside a Vim keystroke: selection and edit hooks must not feed the
    /// changes back to Vim.
    handling: bool,
    /// The last message, shown until the next key.
    message: Option<String>,
    /// The status last published.
    published: Option<VimStatus>,
    /// A `:w` waits for its save to finish (for the message).
    pending_write: bool,
    /// The search last highlighted, with the buffer version and lines.
    highlighted: Option<search::Highlighted>,
}

impl Editor {
    /// Whether this editor is in Vim mode.
    pub fn is_vim_enabled(&self) -> bool {
        self.vim.is_some()
    }

    /// The current Vim mode, if Vim mode is on.
    pub fn vim_mode(&self) -> Option<Mode> {
        self.vim.as_ref().map(|v| v.vim.mode())
    }

    /// Turns Vim mode on or off for this editor (normally it follows
    /// [`vim_mode_enabled`] on each render).
    pub fn set_vim_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled == self.vim.is_some() {
            return;
        }
        if enabled {
            let interceptor = self.add_key_interceptor(0, Rc::new(keys::intercept), cx);
            let mut vim = Vim::new();
            let ctx = self.vim_context(cx);
            vim.sync_selections(&mut self.buffer, &mut self.selections, &ctx);
            self.vim = Some(Box::new(VimState {
                vim,
                interceptor,
                handling: false,
                message: None,
                published: None,
                pending_write: false,
                highlighted: None,
            }));
            self.selections_changed(cx);
        } else if let Some(state) = self.vim.take() {
            self.remove_key_interceptor(state.interceptor);
            self.clear_decorations(search::SEARCH_SET, cx);
            self.set_cursor_shape(CursorShape::Bar, cx);
            cx.emit(crate::EditorEvent::VimStatusChanged);
        }
        self.vim_refresh_cursor(cx);
        self.vim_publish(cx);
        cx.notify();
    }

    /// Flips the session-wide Vim mode toggle; every editor follows on its
    /// next render.
    pub fn toggle_vim_mode(&mut self, cx: &mut Context<Self>) {
        toggle_vim_mode(cx);
        self.vim_sync(cx);
    }

    /// Called on render: follows the setting and the toggle.
    pub(crate) fn vim_sync(&mut self, cx: &mut Context<Self>) {
        let enabled = vim_mode_enabled(cx);
        if enabled != self.vim.is_some() {
            self.set_vim_enabled(enabled, cx);
        }
    }

    /// The status-bar information, if Vim mode is on.
    pub fn vim_status(&self) -> Option<VimStatus> {
        let state = self.vim.as_ref()?;
        let mode = state.vim.mode();
        Some(VimStatus {
            mode: mode.label(),
            kind: mode,
            pending: state.vim.pending_keys(),
            command_line: state.vim.command_line(),
            message: state.message.clone(),
            recording: state.vim.recording_macro(),
        })
    }

    /// Where block cursors are drawn: Vim's cursor char in Visual modes
    /// (the selections alone cannot tell), else the selection heads.
    pub(crate) fn cursor_heads(&self) -> Vec<usize> {
        match &self.vim {
            Some(state) => state.vim.cursor_positions(&self.selections),
            None => self.selections.iter().map(|s| s.head).collect(),
        }
    }

    /// Vim's primary cursor char in Visual modes.
    pub(crate) fn vim_primary_head(&self) -> Option<usize> {
        let state = self.vim.as_ref().filter(|s| s.vim.mode().is_visual())?;
        let heads = state.vim.cursor_positions(&self.selections);
        heads.get(self.selections.primary_index()).copied()
    }

    /// Selections changed outside Vim (mouse, multi-cursor commands): a
    /// drag enters Visual mode, a click leaves it.
    pub(crate) fn vim_after_selections(&mut self, cx: &mut Context<Self>) {
        if self.vim.as_ref().is_none_or(|s| s.handling) {
            return;
        }
        let ctx = self.vim_context(cx);
        let Some(state) = self.vim.as_mut() else {
            return;
        };
        state.handling = true;
        state
            .vim
            .sync_selections(&mut self.buffer, &mut self.selections, &ctx);
        state.handling = false;
        self.vim_refresh_cursor(cx);
        self.vim_publish(cx);
    }

    /// Edits made outside Vim (completion, formatting, paste): keep marks.
    pub(crate) fn vim_after_edits(&mut self, edits: &[arcode_buffer::Edit]) {
        if let Some(state) = self.vim.as_mut().filter(|s| !s.handling) {
            state.vim.on_external_edits(edits);
        }
    }

    /// Sets the cursor shape for the current mode.
    pub(crate) fn vim_refresh_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.vim_mode() else {
            return;
        };
        let shape = match mode.cursor_shape() {
            arcode_vim::CursorShape::Block => CursorShape::Block,
            arcode_vim::CursorShape::Bar => CursorShape::Bar,
            arcode_vim::CursorShape::Underline => CursorShape::Underline,
        };
        self.set_cursor_shape(shape, cx);
    }

    /// Emits [`EditorEvent::VimStatusChanged`](crate::EditorEvent) when
    /// the status changed since it was last published.
    pub(crate) fn vim_publish(&mut self, cx: &mut Context<Self>) {
        let status = self.vim_status();
        let Some(state) = self.vim.as_mut() else {
            return;
        };
        if state.published != status {
            state.published = status;
            cx.emit(crate::EditorEvent::VimStatusChanged);
        }
    }
}

/// Runs `f` with the shared registers swapped into `vim`.
fn with_shared<R>(vim: &mut Vim, cx: &mut App, f: impl FnOnce(&mut Vim) -> R) -> R {
    let mut shared = std::mem::take(&mut cx.default_global::<SharedVim>().0);
    vim.swap_shared(&mut shared);
    let result = f(vim);
    vim.swap_shared(&mut shared);
    cx.default_global::<SharedVim>().0 = shared;
    result
}
