//! [`EditorEvent`]: what an [`Editor`](crate::Editor) reports to its
//! subscribers.

use arcode_buffer::Edit;
use gpui::{Modifiers, SharedString};

use crate::ext::GutterLane;

/// Events emitted by an [`Editor`](crate::Editor).
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
    /// A save was refused because the file changed on disk since the
    /// buffer was loaded or saved (ask, then
    /// [`Editor::save_overwriting`](crate::Editor::save_overwriting)).
    SaveConflict,
    /// [`Editor::disk_conflict`](crate::Editor::disk_conflict) changed.
    DiskConflictChanged,
    /// The editor shows another file ([`Editor::set_path`](crate::Editor::set_path)).
    PathChanged,
    /// Cursors or selections moved.
    SelectionsChanged,
    /// The dirty flag flipped.
    DirtyChanged(bool),
    /// A gutter lane was clicked: on a mark (`tag` is its
    /// [`GutterDecoration::tag`](crate::GutterDecoration::tag)) or on an
    /// empty slot of a lane that is clickable when empty (`tag` is `None`).
    GutterClicked {
        /// Zero-based buffer line.
        line: usize,
        /// The lane.
        lane: GutterLane,
        /// Tag of the clicked mark, if any.
        tag: Option<u64>,
        /// Modifiers held during the click.
        modifiers: Modifiers,
    },
    /// A gutter lane was right-clicked (on a mark or an empty slot of a
    /// lane that is clickable when empty), e.g. to edit a breakpoint.
    GutterContextClicked {
        /// Zero-based buffer line.
        line: usize,
        /// The lane.
        lane: GutterLane,
        /// Tag of the clicked mark, if any.
        tag: Option<u64>,
    },
    /// A code lens item was clicked.
    CodeLensClicked {
        /// The set the lens belongs to.
        set: SharedString,
        /// Zero-based buffer line of the lens.
        line: usize,
        /// The [`LensItem::id`](crate::LensItem::id).
        id: u64,
    },
    /// Vim mode was switched, or its mode, pending keys, command line or
    /// message changed (see [`Editor::vim_status`]).
    VimStatusChanged,
}
