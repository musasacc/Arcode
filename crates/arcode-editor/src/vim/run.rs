//! Running one key through Vim and carrying out what it asks for.

use std::rc::Rc;

use arcode_buffer::ops::OpsConfig;
use arcode_syntax::Language;
use arcode_vim::{Key, Mode, VimAction, VimCommand, VimContext};
use gpui::{ClipboardItem, Context, Pixels};

use super::{VimRequest, with_shared};
use crate::Editor;
use crate::api::ScrollAnchor;
use crate::editor::Autoscroll;
use crate::layout::LayoutSnapshot;

/// The settings Vim needs for one key.
pub(super) fn context(
    cfg: OpsConfig,
    language: &Language,
    layout: &Option<Rc<LayoutSnapshot>>,
) -> VimContext {
    let (top, lines) = match layout {
        Some(l) => {
            let rows = (l.text_bounds.size.height / l.line_height).floor().max(1.);
            (l.visible_range().start, rows as usize)
        }
        None => (0, 40),
    };
    VimContext {
        tab_size: cfg.tab_width,
        indent: cfg.indent,
        viewport_top: top,
        viewport_lines: lines,
        line_comment: language.line_comment.map(str::to_string),
    }
}

impl Editor {
    pub(super) fn vim_context(&self, cx: &gpui::App) -> VimContext {
        context(self.ops_config(cx), self.language, &self.layout)
    }

    /// Feeds `key` to Vim and syncs the editor with what it did. Returns
    /// Vim's action and a request for the application, if any.
    pub(super) fn vim_feed(
        &mut self,
        key: &Key,
        cx: &mut Context<Self>,
    ) -> (VimAction, Option<VimRequest>) {
        let ctx = self.vim_context(cx);
        let Some(state) = self.vim.as_mut() else {
            return (VimAction::PassThrough, None);
        };
        state.vim.set_read_only(self.read_only);
        let before_mode = state.vim.mode();
        let before_sels = self.selections.clone();
        state.handling = true;
        state.message = None;
        let (buffer, sels) = (&mut self.buffer, &mut self.selections);
        let (action, query) = with_shared(&mut state.vim, cx, |vim| {
            let action = vim.handle_key(key, buffer, sels, &ctx);
            (action, vim.search_query().cloned())
        });
        let edits = state.vim.take_edits();
        state.handling = false;
        let typed = match (before_mode, key.key.chars().count()) {
            (Mode::Insert, 1) if !key.ctrl && !key.alt => Some(key.key.clone()),
            _ => None,
        };
        self.vim_sync_changes(edits, before_sels, before_mode, cx);
        if let Some(text) = typed.filter(|_| self.vim_mode() == Some(Mode::Insert)) {
            self.completion_typed(&text, cx);
        }
        let request = self.vim_action(&action, cx);
        self.vim_refresh_cursor(cx);
        self.vim_update_search(query, cx);
        self.vim_publish(cx);
        (action, request)
    }

    /// Edits text the platform committed (IME) in Insert mode through Vim,
    /// so `.` repeats it.
    pub(crate) fn vim_insert_text(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let ctx = self.vim_context(cx);
        let Some(state) = self.vim.as_mut() else {
            return false;
        };
        state.vim.set_read_only(self.read_only);
        let mode = state.vim.mode();
        if !matches!(mode, Mode::Insert | Mode::Replace) {
            // Normal mode never inserts text; treat it as typed keys.
            for c in text.chars() {
                self.vim_feed(&Key::char(c), cx);
            }
            return true;
        }
        let before = self.selections.clone();
        state.handling = true;
        let (buffer, sels) = (&mut self.buffer, &mut self.selections);
        with_shared(&mut state.vim, cx, |vim| {
            vim.insert_text(text, buffer, sels, &ctx)
        });
        let edits = state.vim.take_edits();
        state.handling = false;
        self.vim_sync_changes(edits, before, mode, cx);
        self.completion_typed(text, cx);
        true
    }

    /// Runs the editor's bookkeeping for what Vim changed.
    fn vim_sync_changes(
        &mut self,
        edits: Vec<arcode_buffer::Edit>,
        before: arcode_buffer::SelectionSet,
        before_mode: Mode,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.vim.as_mut() {
            state.handling = true;
        }
        if !edits.is_empty() {
            // Vim refuses changes in a read-only buffer itself
            // (`Vim::set_read_only`), so edits here are always allowed.
            self.after_edits(edits, cx);
        } else if before != self.selections {
            self.selections_changed(cx);
        }
        let mode = self.vim_mode();
        let left_insert = matches!(before_mode, Mode::Insert | Mode::Replace)
            && !matches!(mode, Some(Mode::Insert | Mode::Replace));
        if left_insert {
            self.snippet_cancel();
            self.hide_completions(cx);
            if self.signature_help_visible() {
                self.hide_signature_help(cx);
            }
        }
        if let Some(state) = self.vim.as_mut() {
            state.handling = false;
        }
        cx.notify();
    }

    /// Carries out `action`; returns what the application must do.
    fn vim_action(&mut self, action: &VimAction, cx: &mut Context<Self>) -> Option<VimRequest> {
        match action {
            VimAction::Command(cmd) => return self.vim_command(cmd, cx),
            VimAction::ScrollCursorTop => self.vim_scroll_cursor(ScrollAnchor::Top, cx),
            VimAction::ScrollCursorCenter => self.vim_scroll_cursor(ScrollAnchor::Center, cx),
            VimAction::ScrollCursorBottom => self.vim_scroll_bottom(cx),
            VimAction::ScrollLines(n) => self.vim_scroll_lines(*n, cx),
            VimAction::Yank(text) => cx.write_to_clipboard(ClipboardItem::new_string(text.clone())),
            VimAction::PasteRequest => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                self.vim_paste_clipboard(&text, cx);
            }
            VimAction::StatusMessage(text) => self.vim_message(text.clone()),
            VimAction::None | VimAction::PassThrough | VimAction::ModeChanged(_) => {}
        }
        None
    }

    fn vim_paste_clipboard(&mut self, text: &str, cx: &mut Context<Self>) {
        let ctx = self.vim_context(cx);
        let Some(state) = self.vim.as_mut() else {
            return;
        };
        let mode = state.vim.mode();
        let before = self.selections.clone();
        state.handling = true;
        let (buffer, sels) = (&mut self.buffer, &mut self.selections);
        with_shared(&mut state.vim, cx, |vim| {
            vim.paste_clipboard(text, buffer, sels, &ctx)
        });
        let edits = state.vim.take_edits();
        state.handling = false;
        self.vim_sync_changes(edits, before, mode, cx);
    }

    /// A save finished: report the result of a pending `:w`.
    pub(crate) fn vim_saved(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        let title = self.title();
        let Some(state) = self.vim.as_mut().filter(|s| s.pending_write) else {
            return;
        };
        state.pending_write = false;
        state.message = Some(match result {
            Ok(()) => format!("\"{title}\" written"),
            Err(err) => format!("E212: Can't open file for writing: {err}"),
        });
        self.vim_publish(cx);
        cx.notify();
    }

    pub(super) fn vim_message(&mut self, text: String) {
        if let Some(state) = self.vim.as_mut() {
            state.message = Some(text);
        }
    }

    fn vim_command(&mut self, cmd: &VimCommand, cx: &mut Context<Self>) -> Option<VimRequest> {
        match cmd {
            VimCommand::Save => {
                // The message comes when the save finished (after a
                // format on save): see `vim_saved`.
                if let Some(state) = self.vim.as_mut() {
                    state.pending_write = true;
                }
                self.save_with_format(cx);
            }
            VimCommand::Quit if self.is_dirty() => {
                self.vim_message("E37: No write since last change (add ! to override)".into());
            }
            VimCommand::Quit => return Some(VimRequest::Close { force: false }),
            VimCommand::ForceQuit => return Some(VimRequest::Close { force: true }),
            VimCommand::SaveQuit => {
                if self.is_dirty()
                    && let Err(err) = self.save(cx)
                {
                    self.vim_message(format!("E212: Can't open file for writing: {err}"));
                    return None;
                }
                return Some(VimRequest::Close { force: false });
            }
            VimCommand::Substitute {
                replacements,
                lines,
            } => {
                if *replacements > 1 {
                    let on = if *lines == 1 { "line" } else { "lines" };
                    self.vim_message(format!("{replacements} substitutions on {lines} {on}"));
                }
            }
            VimCommand::GotoLine(_) => {
                self.autoscroll = Some(Autoscroll::Center);
                cx.notify();
            }
            VimCommand::Search(_) | VimCommand::NoHighlight => {}
            VimCommand::Unknown(text) => return self.vim_unknown_command(text),
        }
        None
    }

    /// Ex commands Vim leaves to the editor: `:e <file>`.
    fn vim_unknown_command(&mut self, text: &str) -> Option<VimRequest> {
        let (name, arg) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let arg = arg.trim();
        match name {
            "e" | "edit" | "e!" | "edit!" if !arg.is_empty() => {
                return Some(VimRequest::Open(arg.to_string()));
            }
            "e" | "edit" | "e!" | "edit!" => self.vim_message("E32: No file name".into()),
            _ => self.vim_message(format!("E492: Not an editor command: {text}")),
        }
        None
    }

    fn vim_scroll_cursor(&mut self, anchor: ScrollAnchor, cx: &mut Context<Self>) {
        let line = self.cursor_position().line;
        self.scroll_to_line(line, anchor, cx);
    }

    fn vim_scroll_bottom(&mut self, cx: &mut Context<Self>) {
        let line = self.cursor_position().line;
        let rows = self.vim_context(cx).viewport_lines;
        let top = (line + 1).saturating_sub(rows);
        self.scroll_to_line(top, ScrollAnchor::Top, cx);
    }

    fn vim_scroll_lines(&mut self, n: isize, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.clone() else {
            return;
        };
        let y = self.scroll_y + layout.line_height * n as f32;
        self.scroll_y = y.clamp(Pixels::ZERO, layout.max_scroll_y);
        // Keep the cursor where it is; Vim already moved it into view.
        self.autoscroll = None;
        cx.notify();
    }
}
