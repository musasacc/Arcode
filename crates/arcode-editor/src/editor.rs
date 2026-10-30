//! The [`Editor`] entity: buffer, selections, syntax and diagnostics.

use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use arcode_buffer::{Buffer, Edit, Point, SelectionSet, ops::OpsConfig};
use arcode_syntax::{HighlightSpan, InputEditData, Language, Syntax};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Pixels, SharedString, Task, Window,
};

use crate::diagnostics::{EditorDiagnostic, adjust_for_edit};
use crate::layout::LayoutSnapshot;
use crate::mouse::{DragState, HoverState};
use crate::style::{EditorStyle, ops_config};

/// Events emitted by an [`Editor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    /// The buffer changed. `edits` are in application order (apply them
    /// sequentially); `version` is the buffer version afterwards.
    Edited {
        /// The applied edits.
        edits: Vec<Edit>,
        /// Buffer version after the edits.
        version: u64,
    },
    /// The buffer was written to disk.
    Saved,
    /// Cursors or selections moved.
    SelectionsChanged,
    /// The dirty flag flipped.
    DirtyChanged(bool),
}

/// How a pending scroll request positions the primary cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Autoscroll {
    /// Scroll the minimum needed to show the cursor.
    Fit,
    /// Center the cursor line if it is not visible.
    Center,
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
    pub(crate) cursor_visible: bool,
    pub(crate) blink_epoch: u64,
    pub(crate) blink_task: Option<Task<()>>,
    /// Text of the last whole-line copy, so pasting it inserts lines.
    pub(crate) line_clipboard: Option<String>,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Converts a buffer edit to tree-sitter's input edit.
pub(crate) fn input_edit(e: &Edit) -> InputEditData {
    InputEditData {
        start_byte: e.start_byte,
        old_end_byte: e.old_end_byte,
        new_end_byte: e.new_end_byte,
        start_position: (e.start_position.row, e.start_position.column),
        old_end_position: (e.old_end_position.row, e.old_end_position.column),
        new_end_position: (e.new_end_position.row, e.new_end_position.column),
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
        let syntax = Syntax::new(language, buffer.text());
        let was_dirty = buffer.is_dirty();
        Editor {
            buffer,
            language,
            syntax,
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
            cursor_visible: true,
            blink_epoch: 0,
            blink_task: None,
            line_clipboard: None,
        }
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

    /// Blocks or allows edits.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.notify();
    }

    /// Writes the buffer to its path and emits [`EditorEvent::Saved`].
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        self.buffer.save()?;
        cx.emit(EditorEvent::Saved);
        self.check_dirty(cx);
        cx.notify();
        Ok(())
    }

    /// Replaces all diagnostics.
    pub fn set_diagnostics(&mut self, diagnostics: Vec<EditorDiagnostic>, cx: &mut Context<Self>) {
        self.diagnostics = diagnostics;
        self.hover = None;
        cx.notify();
    }

    /// The current diagnostics (adjusted for edits since they were set).
    pub fn diagnostics(&self) -> &[EditorDiagnostic] {
        &self.diagnostics
    }

    /// Position of the primary cursor.
    pub fn cursor_position(&self) -> Point {
        self.buffer.char_to_point(self.selections.primary().head)
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
        EditorStyle::from_settings(self.language.id, cx)
    }

    /// Editing-command settings for this buffer's language.
    pub(crate) fn ops_config(&self, cx: &App) -> OpsConfig {
        ops_config(self.language.id, cx)
    }

    /// Bookkeeping after the selections moved without an edit.
    pub(crate) fn selections_changed(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
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
        self.apply_edits(edits, cx);
    }

    /// Syncs syntax, diagnostics and listeners after `edits` were applied
    /// to the buffer, and emits the events.
    pub(crate) fn apply_edits(&mut self, edits: Vec<Edit>, cx: &mut Context<Self>) {
        if edits.is_empty() {
            return;
        }
        if let Some(syntax) = self.syntax.as_mut() {
            for e in &edits {
                syntax.edit(&input_edit(e));
            }
            syntax.reparse(self.buffer.text());
        }
        for e in &edits {
            adjust_for_edit(&mut self.diagnostics, e);
        }
        self.highlights = None;
        self.hover = None;
        let version = self.buffer.version();
        cx.emit(EditorEvent::Edited { edits, version });
        self.check_dirty(cx);
        self.autoscroll.get_or_insert(Autoscroll::Fit);
        self.reset_blink(cx);
        cx.emit(EditorEvent::SelectionsChanged);
        cx.notify();
    }

    fn check_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = self.buffer.is_dirty();
        if dirty != self.was_dirty {
            self.was_dirty = dirty;
            cx.emit(EditorEvent::DirtyChanged(dirty));
        }
    }
}
