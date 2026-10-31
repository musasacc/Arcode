//! Public value types: modes, actions, commands and the editor context.

use arcode_buffer::ops::IndentUnit;

/// The Vim mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Normal mode (commands, motions).
    #[default]
    Normal,
    /// Insert mode: typed text is inserted.
    Insert,
    /// Replace mode (`R`): typed text overwrites.
    Replace,
    /// Characterwise visual mode (`v`).
    Visual,
    /// Linewise visual mode (`V`).
    VisualLine,
    /// Blockwise visual mode (`Ctrl-V`), shown as one selection per line.
    VisualBlock,
    /// An operator waits for its motion (`d` typed, `w` not yet).
    OperatorPending,
    /// Typing an ex command (`:`) or a search (`/`, `?`).
    CommandLine,
}

/// How the editor should draw the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// A block covering the char at the cursor.
    Block,
    /// A thin bar between chars.
    Bar,
    /// An underline below the char at the cursor.
    Underline,
}

impl Mode {
    /// True for the three visual modes.
    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine | Mode::VisualBlock)
    }

    /// Status-bar label (`NORMAL`, `INSERT`, `VISUAL`, `V-LINE`, ...).
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Replace => "REPLACE",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::VisualBlock => "V-BLOCK",
            Mode::OperatorPending => "O-PENDING",
            Mode::CommandLine => "COMMAND",
        }
    }

    /// The cursor shape for this mode.
    pub fn cursor_shape(self) -> CursorShape {
        match self {
            Mode::Insert | Mode::CommandLine => CursorShape::Bar,
            Mode::Replace | Mode::OperatorPending => CursorShape::Underline,
            _ => CursorShape::Block,
        }
    }
}

/// An ex command the editor must carry out (or that was carried out here
/// and is reported for information).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimCommand {
    /// `:w`, `:w!`, `:write`.
    Save,
    /// `:q`, `:quit`.
    Quit,
    /// `:wq`, `:x`, `:xit`, `:exit`, `ZZ`.
    SaveQuit,
    /// `:q!`, `ZQ`.
    ForceQuit,
    /// `:s` / `:%s` was applied to the buffer already (one undo step).
    Substitute {
        /// Number of replacements.
        replacements: usize,
        /// Number of lines that changed.
        lines: usize,
    },
    /// `:{n}` (1-based). The cursor was already moved; the editor may
    /// want to center the view.
    GotoLine(usize),
    /// A search was started (`/`, `?`, `*`, `#`): highlight matches of
    /// this literal pattern (see [`crate::Vim::search_query`] for the
    /// whole-word and case flags).
    Search(String),
    /// `:noh`: stop highlighting search matches.
    NoHighlight,
    /// Any other ex command, verbatim (without the leading `:`).
    Unknown(String),
}

/// What the editor should do after [`crate::Vim::handle_key`], beyond the
/// buffer and selection changes already made.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VimAction {
    /// Nothing more to do (the key was consumed).
    #[default]
    None,
    /// Vim did not consume the key: run the editor's own binding for it
    /// (e.g. Ctrl-S, Ctrl-P, Alt combos, completion keys in insert mode).
    PassThrough,
    /// An ex command or a search to carry out / react to.
    Command(VimCommand),
    /// `zt`: scroll so the cursor line is at the top.
    ScrollCursorTop,
    /// `zz`: scroll so the cursor line is centered.
    ScrollCursorCenter,
    /// `zb`: scroll so the cursor line is at the bottom.
    ScrollCursorBottom,
    /// Scroll the view by this many lines (positive = down) without
    /// moving the cursor further (Ctrl-E/Y; Ctrl-D/U/F/B also moved it).
    ScrollLines(isize),
    /// Put this text on the system clipboard (`"+` / `"*` yank/delete).
    Yank(String),
    /// Paste from the system clipboard (`"+p` etc.): read the clipboard
    /// and call [`crate::Vim::paste_clipboard`] with its text.
    PasteRequest,
    /// The mode changed (informational; always read [`crate::Vim::mode`]).
    ModeChanged(Mode),
    /// A message for the status bar (errors such as "Pattern not found").
    StatusMessage(String),
}

/// Editor settings Vim needs for a keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimContext {
    /// Display width of a tab character.
    pub tab_size: usize,
    /// One indentation level (`shiftwidth` / `expandtab`) for `>`, `<`,
    /// `=` and auto-indent in insert mode.
    pub indent: IndentUnit,
    /// First visible line (0-based), for `H`, `M`, `L`.
    pub viewport_top: usize,
    /// Number of visible lines, for `H M L` and `Ctrl-D/U/F/B`.
    pub viewport_lines: usize,
    /// Line comment prefix of the language (`"//"`), for `gc`.
    pub line_comment: Option<String>,
}

impl Default for VimContext {
    fn default() -> Self {
        VimContext {
            tab_size: 4,
            indent: IndentUnit::Spaces(4),
            viewport_top: 0,
            viewport_lines: 40,
            line_comment: None,
        }
    }
}

/// How the text of a register was yanked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterKind {
    /// Part of a line (or lines); pasted inline.
    Charwise,
    /// Whole lines; pasted above/below.
    Linewise,
    /// A visual block; pasted as a block.
    Blockwise,
}

/// The content of a register.
///
/// `pieces` holds one entry per cursor (multi-cursor yank) or per block
/// row. Linewise pieces end with `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    /// Text pieces (see type docs).
    pub pieces: Vec<String>,
    /// How to paste.
    pub kind: RegisterKind,
}

impl Register {
    /// A single-piece register.
    pub fn new(text: impl Into<String>, kind: RegisterKind) -> Self {
        Register {
            pieces: vec![text.into()],
            kind,
        }
    }

    /// The whole text: linewise pieces concatenated, others joined with
    /// `\n`.
    pub fn text(&self) -> String {
        match self.kind {
            RegisterKind::Linewise => self.pieces.concat(),
            _ => self.pieces.join("\n"),
        }
    }
}

/// The active search, for highlighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// The literal pattern.
    pub pattern: String,
    /// Only matches bounded by non-word chars (`*`, `#`).
    pub whole_word: bool,
    /// Case-sensitive (a `\c` in the pattern turns this off).
    pub case_sensitive: bool,
    /// Last search direction was forward.
    pub forward: bool,
}
