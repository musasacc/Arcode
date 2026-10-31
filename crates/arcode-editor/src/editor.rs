//! The [`Editor`] entity: buffer, selections, syntax and diagnostics.

use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use arcode_buffer::{Buffer, Edit, Point, SelectionSet, ops::OpsConfig};
use arcode_completion::SnippetSession;
use arcode_syntax::{HighlightSpan, InputEditData, Language, Syntax};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Pixels, SharedString, Task, Window,
};

use crate::api::ScrollAnchor;
use crate::completion::CompletionState;
use crate::diagnostics::{EditorDiagnostic, adjust_for_edit};
pub use crate::event::EditorEvent;
use crate::ext::ExtState;
use crate::hover::{HoverState, HoverTracker};
use crate::layout::LayoutSnapshot;
use crate::mouse::DragState;
use crate::style::{EditorStyle, ops_config};

/// How a pending scroll request positions the primary cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Autoscroll {
    /// Scroll the minimum needed to show the cursor.
    Fit,
    /// Center the cursor line if it is not visible.
    Center,
    /// Show a given line (from `scroll_to_line`), cursor untouched.
    Line(usize, ScrollAnchor),
}

/// Highlight spans for a range of lines at one buffer version.
pub(crate) struct HighlightCache {
    pub version: u64,
    pub lines: Range<usize>,
    pub spans: Vec<Vec<HighlightSpan>>,
}

/// A code editor view over one [`Buffer`].
pub struct Editor {
    pub(crate) buffer: Buffer,
    pub(crate) language: &'static Language,
    pub(crate) syntax: Option<Syntax>,
    pub(crate) selections: SelectionSet,
    pub(crate) diagnostics: Vec<EditorDiagnostic>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) read_only: bool,
    was_dirty: bool,
    /// Scroll offset in pixels (x: horizontal, y: vertical).
    pub(crate) scroll_x: Pixels,
    pub(crate) scroll_y: Pixels,
    pub(crate) autoscroll: Option<Autoscroll>,
    pub(crate) layout: Option<Rc<LayoutSnapshot>>,
    pub(crate) highlights: Option<HighlightCache>,
    /// IME composition ranges (char ranges, one per selection).
    pub(crate) marked: Option<Vec<Range<usize>>>,
    pub(crate) drag: Option<DragState>,
    pub(crate) hover: Option<HoverState>,
    /// Hover requests and the definition link.
    pub(crate) hovering: HoverTracker,
    pub(crate) cursor_visible: bool,
    pub(crate) blink_epoch: u64,
    pub(crate) blink_task: Option<Task<()>>,
    /// Text of the last whole-line copy, so pasting it inserts lines.
    pub(crate) line_clipboard: Option<String>,
    /// Background syntax job (large buffers), if running.
    pub(crate) syntax_task: Option<Task<()>>,
    /// Edits made while the tree was being reparsed in the background.
    pub(crate) pending_syntax_edits: Vec<InputEditData>,
    /// Large file: no full tree, viewport excerpts are parsed instead.
    pub(crate) large_file: bool,
    /// Decorations, popovers and key handlers from other features.
    pub(crate) ext: ExtState,
    /// The active snippet, if any.
    pub(crate) snippet: Option<SnippetSession>,
    /// Completion list, signature help and their sources.
    pub(crate) completion: CompletionState,
    /// Inlay hints, code actions, rename and formatting.
    pub(crate) lang: crate::lang::LangState,
    /// Between [`Editor::begin_edit_group`] and [`Editor::end_edit_group`]:
    /// the selections the last grouped edit left (`None` before the first).
    pub(crate) edit_group: Option<Option<SelectionSet>>,
    /// Earlier selections for [`crate::UndoCursor`].
    pub(crate) cursor_history: crate::commands::multi_cursor::CursorHistory,
    /// Vim mode state, while Vim mode is on.
    pub(crate) vim: Option<Box<crate::vim::VimState>>,
    /// Indentation chosen for this file (status bar picker or detected
    /// from its content), overriding the language settings.
    pub(crate) indent_override: Option<crate::doc_settings::IndentOverride>,
    /// The file changed or vanished on disk while the buffer kept its own
    /// text (see `reload.rs`).
    pub(crate) disk_conflict: Option<crate::reload::DiskConflict>,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Editor {
    /// Creates an editor for `buffer`. The language comes from the buffer's
    /// path (plain text without one); the buffer is parsed if it has a
    /// grammar.
    pub fn new(buffer: Buffer, cx: &mut Context<Self>) -> Self {
        let language = buffer
            .path()
            .map(arcode_syntax::language_for_path)
            .unwrap_or_else(arcode_syntax::plain_text);
        let was_dirty = buffer.is_dirty();
        let mut editor = Editor {
            buffer,
            language,
            syntax: None,
            selections: SelectionSet::single(0),
            diagnostics: Vec::new(),
            focus_handle: cx.focus_handle(),
            read_only: false,
            was_dirty,
            scroll_x: Pixels::ZERO,
            scroll_y: Pixels::ZERO,
            autoscroll: None,
            layout: None,
            highlights: None,
            marked: None,
            drag: None,
            hover: None,
            hovering: HoverTracker::default(),
            cursor_visible: true,
            blink_epoch: 0,
            blink_task: None,
            line_clipboard: None,
            syntax_task: None,
            pending_syntax_edits: Vec::new(),
            large_file: false,
            ext: ExtState::default(),
            snippet: None,
            completion: CompletionState::default(),
            lang: Default::default(),
            edit_group: None,
            cursor_history: Default::default(),
            vim: None,
            indent_override: None,
            disk_conflict: None,
        };
        crate::completion::register_editor(cx.entity().downgrade(), cx);
        editor.init_syntax(cx);
        editor
    }

    /// Opens `path` in a new editor.
    pub fn open(path: &Path, cx: &mut Context<Self>) -> std::io::Result<Self> {
        let buffer = Buffer::from_file(path)?;
        Ok(Editor::new(buffer, cx))
    }

    /// The buffer being edited.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The buffer's language (plain text if unknown).
    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The file path, if the buffer has one.
    pub fn path(&self) -> Option<&Path> {
        self.buffer.path()
    }

    /// The file name, or `"untitled"`.
    pub fn title(&self) -> SharedString {
        self.path()
            .and_then(|p| p.file_name())
            .map(|n| SharedString::from(n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| SharedString::new_static("untitled"))
    }

    /// Whether the buffer differs from the file on disk.
    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    /// Whether edits are blocked.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Points the editor at another file (it was renamed or moved). The
    /// text and undo history stay; the language follows the new name.
    pub fn set_path(&mut self, path: impl Into<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.buffer.set_path(path);
        let language = self
            .buffer
            .path()
            .map(arcode_syntax::language_for_path)
            .unwrap_or_else(arcode_syntax::plain_text);
        if !std::ptr::eq(language, self.language) {
            self.language = language;
            self.syntax = None;
            self.syntax_task = None;
            self.pending_syntax_edits.clear();
            self.highlights = None;
            self.init_syntax(cx);
        }
        cx.emit(EditorEvent::PathChanged);
        cx.notify();
    }

    /// Blocks or allows edits.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.notify();
    }

    /// Writes the buffer to its path and emits [`EditorEvent::Saved`].
    /// A read-only editor (library sources, diffs) never writes its file,
    /// and a file another program changed since it was loaded or saved is
    /// not overwritten: that emits [`EditorEvent::SaveConflict`] instead
    /// (see [`Editor::save_overwriting`]).
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        if self.read_only {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the editor is read-only",
            ));
        }
        self.check_save_conflict(cx)?;
        self.buffer.save()?;
        self.set_disk_conflict(None, cx);
        if self
            .path()
            .is_some_and(crate::completion::snippets::is_snippets_file)
        {
            crate::completion::reload_user_snippets(cx);
        }
        cx.emit(EditorEvent::Saved);
        self.check_dirty(cx);
        cx.notify();
        Ok(())
    }

    /// Replaces all diagnostics.
    pub fn set_diagnostics(&mut self, diagnostics: Vec<EditorDiagnostic>, cx: &mut Context<Self>) {
        self.diagnostics = diagnostics;
        self.hover_after_diagnostics();
        cx.notify();
    }

    /// The current diagnostics (adjusted for edits since they were set).
    pub fn diagnostics(&self) -> &[EditorDiagnostic] {
        &self.diagnostics
    }

    /// Position of the primary cursor.
    /// In Vim's Visual modes this is the char Vim's cursor is on.
    pub fn cursor_position(&self) -> Point {
        let head = self
            .vim_primary_head()
            .unwrap_or(self.selections.primary().head);
        self.buffer.char_to_point(head)
    }

    /// The current selections.
    pub fn selections(&self) -> &SelectionSet {
        &self.selections
    }

    /// Buffer lines visible in the last painted frame (empty before the
    /// first frame).
    pub fn visible_line_range(&self) -> Range<usize> {
        self.layout
            .as_ref()
            .map_or(0..0, |layout| layout.visible_range())
    }

    /// Replaces the selections (clamped to the buffer).
    pub fn set_selections(&mut self, mut selections: SelectionSet, cx: &mut Context<Self>) {
        selections.clamp(self.buffer.len_chars());
        self.selections = selections;
        self.selections_changed(cx);
    }

    /// Moves the cursor to `point` (clamped), scrolls it into view (centered
    /// if it was off-screen) and focuses the editor.
    pub fn go_to(&mut self, point: Point, window: &mut Window, cx: &mut Context<Self>) {
        let line = point.line.min(self.buffer.line_count().saturating_sub(1));
        let column = point.column.min(self.buffer.line_len(line));
        let pos = self.buffer.point_to_char(Point::new(line, column));
        self.selections = SelectionSet::single(pos);
        self.buffer.break_undo_group();
        self.autoscroll = Some(Autoscroll::Center);
        window.focus(&self.focus_handle);
        self.selections_changed(cx);
    }

    /// The editor's current style from settings.
    pub(crate) fn style(&self, cx: &App) -> EditorStyle {
        let mut style = EditorStyle::from_settings(self.language.id, cx);
        if let Some(indent) = self.indent_override {
            style.tab_size = indent.tab_width;
        }
        style
    }

    /// Editing-command settings for this buffer's language.
    pub(crate) fn ops_config(&self, cx: &App) -> OpsConfig {
        let mut cfg = ops_config(self.language.id, cx);
        if let Some(indent) = self.indent_override {
            cfg.indent = indent.unit;
            cfg.tab_width = indent.tab_width;
        }
        cfg
    }

    /// Bookkeeping after the selections moved without an edit.
    pub(crate) fn selections_changed(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.snippet_check_cursor();
        self.hover_after_cursor_move();
        self.completion_sync(cx);
        self.signature_refresh(cx);
        self.lang_after_cursor_move(cx);
        self.vim_after_selections(cx);
        self.cursor_history.record(&self.selections);
        self.autoscroll.get_or_insert(Autoscroll::Fit);
        self.reset_blink(cx);
        cx.emit(EditorEvent::SelectionsChanged);
        cx.notify();
    }

    /// Runs an editing command unless read-only, then syncs everything.
    pub(crate) fn edit_with(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Buffer, &mut SelectionSet, &OpsConfig) -> Vec<Edit>,
    ) {
        if self.read_only {
            return;
        }
        let cfg = self.ops_config(cx);
        let before = self.selections.clone();
        let edits = f(&mut self.buffer, &mut self.selections, &cfg);
        if edits.is_empty() {
            if before != self.selections {
                self.selections_changed(cx);
            }
            return;
        }
        self.after_edits(edits, cx);
    }

    /// Syncs syntax, diagnostics and listeners after `edits` were applied
    /// to the buffer, and emits the events.
    pub(crate) fn after_edits(&mut self, mut edits: Vec<Edit>, cx: &mut Context<Self>) {
        if edits.is_empty() {
            return;
        }
        let mirrors = self.snippet_after_edits(&edits);
        edits.extend(mirrors);
        self.snippet_check_cursor();
        self.sync_syntax(&edits, cx);
        for e in &edits {
            adjust_for_edit(&mut self.diagnostics, e);
            self.ext.adjust_for_edit(e);
        }
        self.vim_after_edits(&edits);
        self.cursor_history.reset(&self.selections);
        self.words_after_edits(&edits);
        self.completion_sync(cx);
        self.signature_refresh(cx);
        self.hide_hover();
        self.lang_after_edits(cx);
        let version = self.buffer.version();
        cx.emit(EditorEvent::Edited { edits, version });
        self.check_dirty(cx);
        self.autoscroll.get_or_insert(Autoscroll::Fit);
        self.reset_blink(cx);
        cx.emit(EditorEvent::SelectionsChanged);
        cx.notify();
    }

    pub(crate) fn check_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = self.buffer.is_dirty();
        if dirty != self.was_dirty {
            self.was_dirty = dirty;
            cx.emit(EditorEvent::DirtyChanged(dirty));
        }
    }
}
