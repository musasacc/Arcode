//! Monochrome SVG icons tinted with the current text color.

use gpui::{
    App, Hsla, InteractiveElement, IntoElement, Pixels, RenderOnce, SharedString, Styled, Window,
    prelude::FluentBuilder, px, svg,
};

/// Every icon shipped with Arcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    ArrowDown,
    ArrowDownToDot,
    ArrowRight,
    ArrowUp,
    ArrowUpFromDot,
    Box,
    Braces,
    Bug,
    CaseSensitive,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronsDownUp,
    CircleAlert,
    CircleCheck,
    CircleDot,
    CirclePlay,
    CircleX,
    /// `</>`: for code contexts only (the logo is [`IconName::LogoArc`]
    /// plus [`IconName::Ember`]).
    CodeXml,
    Columns,
    Command,
    Copy,
    Ellipsis,
    /// The ember mark: a diamond, "you are here" (tint it `accent`).
    Ember,
    ExternalLink,
    File,
    FileText,
    Files,
    Folder,
    FolderOpen,
    GitBranch,
    GitCommit,
    GitCompare,
    GitMerge,
    History,
    Info,
    Keyboard,
    Lightbulb,
    ListTodo,
    Loader,
    /// The arc of the Arcode logo (tint it `text`; the spark is
    /// [`IconName::Ember`]).
    LogoArc,
    Minus,
    Monitor,
    Moon,
    Package,
    Palette,
    PanelBottom,
    PanelLeft,
    Pause,
    Play,
    Plus,
    Puzzle,
    RedoDot,
    Refresh,
    Regex,
    Replace,
    ReplaceAll,
    RotateCcw,
    Rows,
    Save,
    Search,
    Settings,
    Square,
    SquareFunction,
    Sun,
    Terminal,
    Trash,
    TriangleAlert,
    Type,
    Undo,
    Variable,
    WholeWord,
    X,
}

impl IconName {
    /// Asset path served by [`crate::Assets`].
    pub fn path(self) -> &'static str {
        match self {
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
            Self::Box => "icons/box.svg",
            Self::Braces => "icons/braces.svg",
            Self::Bug => "icons/bug.svg",
            Self::CaseSensitive => "icons/case-sensitive.svg",
            Self::Check => "icons/check.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronsDownUp => "icons/chevrons-down-up.svg",
            Self::CircleAlert => "icons/circle-alert.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::CircleDot => "icons/circle-dot.svg",
            Self::CirclePlay => "icons/circle-play.svg",
            Self::CircleX => "icons/circle-x.svg",
            Self::CodeXml => "icons/code-xml.svg",
            Self::Columns => "icons/columns-2.svg",
            Self::Command => "icons/command.svg",
            Self::Copy => "icons/copy.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::Ember => "icons/ember.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::File => "icons/file.svg",
            Self::FileText => "icons/file-text.svg",
            Self::Files => "icons/files.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::GitBranch => "icons/git-branch.svg",
            Self::GitCommit => "icons/git-commit-horizontal.svg",
            Self::GitCompare => "icons/git-compare.svg",
            Self::GitMerge => "icons/git-merge.svg",
            Self::History => "icons/history.svg",
            Self::Info => "icons/info.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::Lightbulb => "icons/lightbulb.svg",
            Self::ListTodo => "icons/list-todo.svg",
            Self::Loader => "icons/loader.svg",
            Self::LogoArc => "icons/logo-arc.svg",
            Self::Minus => "icons/minus.svg",
            Self::Monitor => "icons/monitor.svg",
            Self::Moon => "icons/moon.svg",
            Self::Package => "icons/package.svg",
            Self::Palette => "icons/palette.svg",
            Self::PanelBottom => "icons/panel-bottom.svg",
            Self::PanelLeft => "icons/panel-left.svg",
            Self::Play => "icons/play.svg",
            Self::Plus => "icons/plus.svg",
            Self::Puzzle => "icons/puzzle.svg",
            Self::Refresh => "icons/refresh-cw.svg",
            Self::Regex => "icons/regex.svg",
            Self::Replace => "icons/replace.svg",
            Self::ReplaceAll => "icons/replace-all.svg",
            Self::Rows => "icons/rows-2.svg",
            Self::Save => "icons/save.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::Square => "icons/square.svg",
            Self::SquareFunction => "icons/square-function.svg",
            Self::Sun => "icons/sun.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Trash => "icons/trash-2.svg",
            Self::TriangleAlert => "icons/triangle-alert.svg",
            Self::Type => "icons/type.svg",
            Self::Undo => "icons/undo-2.svg",
            Self::Variable => "icons/variable.svg",
            Self::WholeWord => "icons/whole-word.svg",
            Self::ArrowDownToDot => "icons/arrow-down-to-dot.svg",
            Self::ArrowUpFromDot => "icons/arrow-up-from-dot.svg",
            Self::Pause => "icons/pause.svg",
            Self::RedoDot => "icons/redo-dot.svg",
            Self::RotateCcw => "icons/rotate-ccw.svg",
            Self::X => "icons/x.svg",
        }
    }
}

/// An icon element. Defaults to 16px and the inherited text color.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Option<Hsla>,
    group_hover: Option<(SharedString, Hsla)>,
}

/// Creates an [`Icon`].
pub fn icon(name: IconName) -> Icon {
    Icon {
        name,
        size: px(16.),
        color: None,
        group_hover: None,
    }
}

impl Icon {
    /// Side length (16 by default; DESIGN.md 5.1 lists the sizes).
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Tint (default `text_muted`).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Switches to `color` while the mouse is over the element `.group(group)`
    /// (an SVG does not inherit its parent's text color).
    pub fn group_hover_color(mut self, group: impl Into<SharedString>, color: Hsla) -> Self {
        self.group_hover = Some((group.into(), color));
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self
            .color
            .unwrap_or_else(|| crate::theme(cx).colors.text_muted);
        svg()
            .path(SharedString::new_static(self.name.path()))
            .size(self.size)
            .flex_none()
            .text_color(color)
            .when_some(self.group_hover, |el, (group, color)| {
                el.group_hover(group, move |s| s.text_color(color))
            })
    }
}
