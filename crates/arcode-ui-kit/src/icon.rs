//! Monochrome SVG icons tinted with the current text color.

use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, SharedString, Styled, Window, px, svg};

/// Every icon shipped with Arcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    ArrowRight,
    Bug,
    ChevronDown,
    ChevronRight,
    CircleAlert,
    CircleCheck,
    CircleDot,
    CirclePlay,
    CircleX,
    Columns,
    Copy,
    Ellipsis,
    ExternalLink,
    File,
    FileText,
    Files,
    Folder,
    FolderOpen,
    GitBranch,
    GitCommit,
    History,
    Info,
    Keyboard,
    ListTodo,
    Loader,
    Minus,
    Monitor,
    Moon,
    Package,
    Palette,
    PanelBottom,
    PanelLeft,
    Play,
    Plus,
    Puzzle,
    Refresh,
    Rows,
    Save,
    Search,
    Settings,
    Square,
    Sun,
    Terminal,
    Trash,
    TriangleAlert,
    Type,
    X,
}

impl IconName {
    /// Asset path served by [`crate::Assets`].
    pub fn path(self) -> &'static str {
        match self {
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::Bug => "icons/bug.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::CircleAlert => "icons/circle-alert.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::CircleDot => "icons/circle-dot.svg",
            Self::CirclePlay => "icons/circle-play.svg",
            Self::CircleX => "icons/circle-x.svg",
            Self::Columns => "icons/columns-2.svg",
            Self::Copy => "icons/copy.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::File => "icons/file.svg",
            Self::FileText => "icons/file-text.svg",
            Self::Files => "icons/files.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::GitBranch => "icons/git-branch.svg",
            Self::GitCommit => "icons/git-commit-horizontal.svg",
            Self::History => "icons/history.svg",
            Self::Info => "icons/info.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::ListTodo => "icons/list-todo.svg",
            Self::Loader => "icons/loader.svg",
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
            Self::Rows => "icons/rows-2.svg",
            Self::Save => "icons/save.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::Square => "icons/square.svg",
            Self::Sun => "icons/sun.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Trash => "icons/trash-2.svg",
            Self::TriangleAlert => "icons/triangle-alert.svg",
            Self::Type => "icons/type.svg",
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
}

/// Creates an [`Icon`].
pub fn icon(name: IconName) -> Icon {
    Icon {
        name,
        size: px(16.),
        color: None,
    }
}

impl Icon {
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
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
    }
}
