//! Editor extension points: public types for decorations, popovers, key
//! interception and cursor shape.
//!
//! Features outside the editor (git, debugger, run buttons, completion,
//! Vim, search, merge conflicts, plugins) describe what they want drawn with
//! these plain data types and hand them to the [`Editor`] through its
//! `set_*` methods. Each owner uses its own **set key** (any
//! string such as `"git"` or `"debugger"`), so owners replace only their
//! own decorations. Positions are buffer coordinates ([`Point`]: zero-based
//! line and char column) and follow edits until the owner replaces them.
//! Colors are theme keys ([`ColorKey`]), never literal colors.

mod blocks;
pub(crate) mod keys;
mod store;

use std::rc::Rc;

use arcode_buffer::Point;
use arcode_ui_kit::{ColorKey, IconName};
use gpui::{AnyView, App, Entity, Keystroke, SharedString, Window};

pub(crate) use blocks::{BlockMap, RowKind};
pub(crate) use store::{ExtState, LensTarget};

use crate::Editor;

/// A vertical lane in the gutter. Lanes are drawn left to right in this
/// order; a lane takes space only while it has decorations or was made
/// visible with [`Editor::set_gutter_lane_visible`](crate::Editor::set_gutter_lane_visible).
///
/// Left of the line numbers: `Breakpoints`, `Runnables`, then the built-in
/// diagnostic marker. Right of the line numbers: `Changes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GutterLane {
    /// Breakpoint dots. Clicking anywhere in this lane emits
    /// [`EditorEvent::GutterClicked`](crate::EditorEvent::GutterClicked),
    /// even on lines without a mark, so breakpoints can be added.
    Breakpoints,
    /// Run / debug buttons (e.g. a play triangle next to tests).
    Runnables,
    /// Thin version-control change bars.
    Changes,
}

impl GutterLane {
    /// All lanes in drawing order.
    pub const ALL: [GutterLane; 3] = [
        GutterLane::Breakpoints,
        GutterLane::Runnables,
        GutterLane::Changes,
    ];

    /// Whether clicking an empty slot in this lane emits an event.
    pub fn clickable_when_empty(self) -> bool {
        self == GutterLane::Breakpoints
    }
}

/// Kind of a version-control change bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// Lines added (theme key `diff_added`).
    Added,
    /// Lines modified (`diff_modified`).
    Modified,
    /// Lines deleted below this line (`diff_deleted`, drawn as a wedge).
    Deleted,
}

impl ChangeKind {
    /// The theme color key for this kind.
    pub fn color_key(self) -> ColorKey {
        ColorKey::new(match self {
            ChangeKind::Added => "diff_added",
            ChangeKind::Modified => "diff_modified",
            ChangeKind::Deleted => "diff_deleted",
        })
    }
}

/// How a gutter dot is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DotStyle {
    /// A filled dot (breakpoint).
    Filled,
    /// A ring (e.g. a disabled or unverified breakpoint).
    Ring,
    /// A filled dot with a small hole (conditional breakpoint).
    Conditional,
}

/// What a gutter decoration shows.
#[derive(Debug, Clone, PartialEq)]
pub enum GutterMark {
    /// A version-control change bar.
    Change(ChangeKind),
    /// An icon such as [`IconName::Play`].
    Icon {
        /// The icon.
        icon: IconName,
        /// Its color.
        color: ColorKey,
    },
    /// A round dot (breakpoints).
    Dot {
        /// Fill color.
        color: ColorKey,
        /// Shape.
        style: DotStyle,
    },
}

/// One mark in a gutter lane on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct GutterDecoration {
    /// Zero-based buffer line.
    pub line: usize,
    /// The lane it is drawn in.
    pub lane: GutterLane,
    /// What is drawn.
    pub mark: GutterMark,
    /// Caller data returned in click events (e.g. a runnable index).
    pub tag: u64,
}

/// What a background highlight covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightRange {
    /// Whole lines (end exclusive), across the full editor width.
    Lines(std::ops::Range<usize>),
    /// A text range, painted behind the glyphs like a selection.
    Text(std::ops::Range<Point>),
}

/// A background color behind lines or text (conflict regions, the
/// debugger's current line, search matches).
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundHighlight {
    /// What is covered.
    pub range: HighlightRange,
    /// The color, usually with reduced opacity.
    pub color: ColorKey,
}

impl BackgroundHighlight {
    /// Highlights whole `lines`.
    pub fn lines(lines: std::ops::Range<usize>, color: ColorKey) -> Self {
        BackgroundHighlight {
            range: HighlightRange::Lines(lines),
            color,
        }
    }

    /// Highlights the text in `range`.
    pub fn text(range: std::ops::Range<Point>, color: ColorKey) -> Self {
        BackgroundHighlight {
            range: HighlightRange::Text(range),
            color,
        }
    }
}

/// Where virtual text appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualTextPosition {
    /// After the end of a line, separated by a small gap (inline blame).
    EndOfLine(usize),
    /// Inside a line, before the char at this position (inlay hints).
    Inline(Point),
}

/// Text drawn in the editor that is not part of the buffer: it cannot be
/// selected, the cursor skips over it and reported columns ignore it.
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualText {
    /// Where it is drawn.
    pub position: VirtualTextPosition,
    /// The text (one line).
    pub text: SharedString,
    /// Text color.
    pub color: ColorKey,
    /// Optional pill background (inlay hints).
    pub background: Option<ColorKey>,
    /// Draw in italics.
    pub italic: bool,
}

impl VirtualText {
    /// Muted text after the end of `line`.
    pub fn end_of_line(line: usize, text: impl Into<SharedString>) -> Self {
        VirtualText {
            position: VirtualTextPosition::EndOfLine(line),
            text: text.into(),
            color: ColorKey::new("text_muted"),
            background: None,
            italic: false,
        }
    }

    /// An inlay hint before the char at `at`.
    pub fn inline(at: Point, text: impl Into<SharedString>) -> Self {
        VirtualText {
            position: VirtualTextPosition::Inline(at),
            text: text.into(),
            color: ColorKey::new("text_muted"),
            background: Some(ColorKey::new("hover")),
            italic: false,
        }
    }
}

/// Where a code lens row appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LensPlacement {
    /// On its own row above the line, aligned with the line's indentation.
    Above,
    /// After the end of the line.
    EndOfLine,
}

/// One clickable label in a code lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensItem {
    /// Caller id returned in [`EditorEvent::CodeLensClicked`](crate::EditorEvent::CodeLensClicked).
    pub id: u64,
    /// Label, e.g. `"Accept Current"` or `"Run"`.
    pub label: SharedString,
}

/// A row of clickable labels (`Run | Debug`) tied to a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLens {
    /// Zero-based buffer line.
    pub line: usize,
    /// Above the line or at its end.
    pub placement: LensPlacement,
    /// The labels, separated by `|` when drawn.
    pub items: Vec<LensItem>,
}

/// The shape of the text cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// A thin vertical bar (default).
    #[default]
    Bar,
    /// A block over the character (Vim normal mode).
    Block,
    /// An underline below the character (Vim replace mode).
    Underline,
    /// A block outline.
    Hollow,
}

/// Called with each keystroke while the editor is focused, before its key
/// bindings run. Return `true` to consume the keystroke (no action and no
/// text input happens for it).
pub type KeyInterceptor = Rc<dyn Fn(&Entity<Editor>, &Keystroke, &mut Window, &mut App) -> bool>;

/// Identifies an installed [`KeyInterceptor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterceptorId(pub(crate) u64);

/// Which side of the anchor a popover opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverPlacement {
    /// Below the anchor line (completion, hover).
    #[default]
    Below,
    /// Above the anchor line (signature help).
    Above,
}

/// A floating view anchored at a buffer position.
///
/// The editor keeps keyboard focus, so typing continues in the buffer.
/// `on_key` (if any) sees every keystroke first while the popover is drawn,
/// which lets a list take up/down/enter/escape. A popover whose anchor line
/// is scrolled out of view is not drawn and gets no keys until it is back
/// in view.
#[derive(Clone)]
pub struct EditorPopover {
    /// Buffer position the popover is attached to (follows edits).
    pub anchor: Point,
    /// The content, built by the caller. It should `occlude()` its bounds.
    pub content: AnyView,
    /// Side of the anchor.
    pub placement: PopoverPlacement,
    /// A panel drawn next to `content` (right of it, or left when there is
    /// no room), aligned with its edge nearest the anchor line, e.g. the
    /// documentation of the selected completion.
    pub side: Option<AnyView>,
    /// Key handler active while the popover is shown.
    pub on_key: Option<KeyInterceptor>,
}

impl EditorPopover {
    /// A popover below `anchor` showing `content`.
    pub fn new(anchor: Point, content: AnyView) -> Self {
        EditorPopover {
            anchor,
            content,
            placement: PopoverPlacement::Below,
            side: None,
            on_key: None,
        }
    }

    /// Draws `side` next to the content.
    pub fn side(mut self, side: AnyView) -> Self {
        self.side = Some(side);
        self
    }

    /// Opens above the anchor instead.
    pub fn above(mut self) -> Self {
        self.placement = PopoverPlacement::Above;
        self
    }

    /// Routes keys to `f` first while shown.
    pub fn on_key(
        mut self,
        f: impl Fn(&Entity<Editor>, &Keystroke, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_key = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for EditorPopover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorPopover")
            .field("anchor", &self.anchor)
            .field("placement", &self.placement)
            .field("side", &self.side.is_some())
            .field("on_key", &self.on_key.is_some())
            .finish_non_exhaustive()
    }
}
