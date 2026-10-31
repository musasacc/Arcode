//! Editor actions and their default key bindings (context `"Editor"`).

use gpui::{App, KeyBinding, actions};

actions!(
    editor,
    [
        /// Moves the cursors one grapheme left.
        MoveLeft,
        /// Moves the cursors one grapheme right.
        MoveRight,
        /// Moves the cursors one line up.
        MoveUp,
        /// Moves the cursors one line down.
        MoveDown,
        /// Extends the selections one grapheme left.
        SelectLeft,
        /// Extends the selections one grapheme right.
        SelectRight,
        /// Extends the selections one line up.
        SelectUp,
        /// Extends the selections one line down.
        SelectDown,
        /// Moves to the previous word start.
        MoveWordLeft,
        /// Moves to the next word end.
        MoveWordRight,
        /// Extends to the previous word start.
        SelectWordLeft,
        /// Extends to the next word end.
        SelectWordRight,
        /// Smart Home: first non-blank char, then column 0.
        MoveHome,
        /// End of line.
        MoveEnd,
        /// Smart Home, extending the selection.
        SelectHome,
        /// End of line, extending the selection.
        SelectEnd,
        /// One page up.
        PageUp,
        /// One page down.
        PageDown,
        /// One page up, extending the selection.
        SelectPageUp,
        /// One page down, extending the selection.
        SelectPageDown,
        /// Start of the document.
        MoveDocStart,
        /// End of the document.
        MoveDocEnd,
        /// Extends to the start of the document.
        SelectDocStart,
        /// Extends to the end of the document.
        SelectDocEnd,
        /// Deletes the selection or the grapheme before the cursor.
        Backspace,
        /// Deletes the selection or the grapheme after the cursor.
        Delete,
        /// Deletes to the previous word start.
        DeleteWordLeft,
        /// Deletes to the next word end.
        DeleteWordRight,
        /// Inserts a line break with indentation.
        Newline,
        /// Indents the selection or inserts one indent unit.
        Tab,
        /// Outdents the selected lines.
        Outdent,
        /// Undoes the last change.
        Undo,
        /// Redoes the last undone change.
        Redo,
        /// Copies the selections (the line if empty).
        Copy,
        /// Cuts the selections (the line if empty).
        Cut,
        /// Pastes the clipboard.
        Paste,
        /// Selects the whole buffer.
        SelectAll,
        /// Selects the word or adds the next occurrence.
        SelectNextOccurrence,
        /// Selects every occurrence of the selection (or of the word at
        /// the cursor).
        SelectAllOccurrences,
        /// Goes back to the cursors and selections before the last cursor
        /// change.
        UndoCursor,
        /// Adds a cursor on the line above.
        AddCursorAbove,
        /// Adds a cursor on the line below.
        AddCursorBelow,
        /// Collapses to a single cursor.
        Cancel,
        /// Toggles a line comment on the selected lines.
        ToggleLineComment,
        /// Moves the selected lines up.
        MoveLineUp,
        /// Moves the selected lines down.
        MoveLineDown,
        /// Duplicates the selected lines.
        DuplicateLine,
        /// Saves the buffer to its file.
        Save,
        /// Opens the completion list at the cursor.
        ShowCompletions,
        /// Shows the parameter hints of the call around the cursor.
        ShowSignatureHelp,
        /// Shows the hover (documentation and diagnostics) at the cursor.
        ShowHover,
        /// Goes to the definition of the symbol at the cursor. Handled by
        /// the application (it may open another file).
        GoToDefinition,
        /// Goes to the type definition of the symbol at the cursor.
        GoToTypeDefinition,
        /// Goes to the implementations of the symbol at the cursor.
        GoToImplementation,
        /// Lists every reference to the symbol at the cursor.
        FindAllReferences,
        /// Shows the code actions (quick fixes, refactorings, source
        /// actions) at the cursor.
        ShowCodeActions,
        /// Shows only the refactorings at the cursor.
        Refactor,
        /// Renames the symbol at the cursor in every file.
        RenameSymbol,
        /// Formats the whole document.
        FormatDocument,
        /// Formats the selection (or the cursor line).
        FormatSelection,
        /// Shows or hides inlay hints in every editor (for this session).
        ToggleInlayHints,
        /// Opens the editor's context menu at the cursor.
        ShowContextMenu,
        /// Turns Vim mode on or off in every editor (for this session).
        ToggleVimMode,
    ]
);

/// The key context editor bindings live in.
pub const KEY_CONTEXT: &str = "Editor";

/// Registers the default key bindings.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    let mut b = vec![
        KeyBinding::new("left", MoveLeft, ctx),
        KeyBinding::new("right", MoveRight, ctx),
        KeyBinding::new("up", MoveUp, ctx),
        KeyBinding::new("down", MoveDown, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("home", MoveHome, ctx),
        KeyBinding::new("end", MoveEnd, ctx),
        KeyBinding::new("shift-home", SelectHome, ctx),
        KeyBinding::new("shift-end", SelectEnd, ctx),
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
        KeyBinding::new("shift-pageup", SelectPageUp, ctx),
        KeyBinding::new("shift-pagedown", SelectPageDown, ctx),
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("shift-backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("enter", Newline, ctx),
        KeyBinding::new("shift-enter", Newline, ctx),
        KeyBinding::new("tab", Tab, ctx),
        KeyBinding::new("shift-tab", Outdent, ctx),
        KeyBinding::new("secondary-z", Undo, ctx),
        KeyBinding::new("secondary-shift-z", Redo, ctx),
        KeyBinding::new("secondary-y", Redo, ctx),
        KeyBinding::new("secondary-c", Copy, ctx),
        KeyBinding::new("secondary-x", Cut, ctx),
        KeyBinding::new("secondary-v", Paste, ctx),
        KeyBinding::new("secondary-a", SelectAll, ctx),
        KeyBinding::new("secondary-d", SelectNextOccurrence, ctx),
        KeyBinding::new("secondary-shift-l", SelectAllOccurrences, ctx),
        KeyBinding::new("secondary-u", UndoCursor, ctx),
        KeyBinding::new("secondary-alt-up", AddCursorAbove, ctx),
        KeyBinding::new("secondary-alt-down", AddCursorBelow, ctx),
        KeyBinding::new("alt-shift-up", AddCursorAbove, ctx),
        KeyBinding::new("alt-shift-down", AddCursorBelow, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("secondary-/", ToggleLineComment, ctx),
        KeyBinding::new("alt-up", MoveLineUp, ctx),
        KeyBinding::new("alt-down", MoveLineDown, ctx),
        KeyBinding::new("secondary-shift-d", DuplicateLine, ctx),
        KeyBinding::new("secondary-s", Save, ctx),
        // Ctrl on every platform (Cmd+Space is the system search on macOS).
        KeyBinding::new("ctrl-space", ShowCompletions, ctx),
        KeyBinding::new("ctrl-shift-space", ShowSignatureHelp, ctx),
        KeyBinding::new("secondary-k secondary-i", ShowHover, ctx),
        KeyBinding::new("f12", GoToDefinition, ctx),
        KeyBinding::new("secondary-f12", GoToImplementation, ctx),
        KeyBinding::new("shift-f12", FindAllReferences, ctx),
        KeyBinding::new("secondary-.", ShowCodeActions, ctx),
        KeyBinding::new("secondary-shift-r", Refactor, ctx),
        KeyBinding::new("f2", RenameSymbol, ctx),
        KeyBinding::new("shift-alt-f", FormatDocument, ctx),
        KeyBinding::new("secondary-k secondary-f", FormatSelection, ctx),
        KeyBinding::new("shift-f10", ShowContextMenu, ctx),
        KeyBinding::new("menu", ShowContextMenu, ctx),
    ];
    if cfg!(target_os = "macos") {
        b.extend([
            KeyBinding::new("alt-left", MoveWordLeft, ctx),
            KeyBinding::new("alt-right", MoveWordRight, ctx),
            KeyBinding::new("alt-shift-left", SelectWordLeft, ctx),
            KeyBinding::new("alt-shift-right", SelectWordRight, ctx),
            KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
            KeyBinding::new("alt-delete", DeleteWordRight, ctx),
            KeyBinding::new("cmd-left", MoveHome, ctx),
            KeyBinding::new("cmd-right", MoveEnd, ctx),
            KeyBinding::new("cmd-shift-left", SelectHome, ctx),
            KeyBinding::new("cmd-shift-right", SelectEnd, ctx),
            KeyBinding::new("cmd-up", MoveDocStart, ctx),
            KeyBinding::new("cmd-down", MoveDocEnd, ctx),
            KeyBinding::new("cmd-shift-up", SelectDocStart, ctx),
            KeyBinding::new("cmd-shift-down", SelectDocEnd, ctx),
            KeyBinding::new("cmd-backspace", DeleteWordLeft, ctx),
            KeyBinding::new("ctrl-a", MoveHome, ctx),
            KeyBinding::new("ctrl-e", MoveEnd, ctx),
        ]);
    } else {
        b.extend([
            KeyBinding::new("ctrl-left", MoveWordLeft, ctx),
            KeyBinding::new("ctrl-right", MoveWordRight, ctx),
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, ctx),
            KeyBinding::new("ctrl-shift-right", SelectWordRight, ctx),
            KeyBinding::new("ctrl-backspace", DeleteWordLeft, ctx),
            KeyBinding::new("ctrl-delete", DeleteWordRight, ctx),
            KeyBinding::new("ctrl-home", MoveDocStart, ctx),
            KeyBinding::new("ctrl-end", MoveDocEnd, ctx),
            KeyBinding::new("ctrl-shift-home", SelectDocStart, ctx),
            KeyBinding::new("ctrl-shift-end", SelectDocEnd, ctx),
            KeyBinding::new("shift-delete", Cut, ctx),
            KeyBinding::new("ctrl-insert", Copy, ctx),
            KeyBinding::new("shift-insert", Paste, ctx),
        ]);
    }
    cx.bind_keys(b);
}
