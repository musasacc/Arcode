//! The project file tree: a lazily loaded, expandable list of folders and
//! files rendered with a `uniform_list` (only visible rows are built).
//!
//! Keyboard: arrows move and expand/collapse, Enter opens, F2 renames,
//! Delete deletes, Shift+F10 (or the context-menu key) opens the context
//! menu. The context menu ([`menu`]) offers New File / New Folder (typed
//! inline in the tree), Rename (inline), Delete (to the trash, with
//! confirmation), Copy Path, Copy Relative Path, Reveal in the system file
//! manager and Open in Terminal ([`ops`]). Expanded folders are watched
//! ([`watch`]) so changes made outside Arcode show up. Rows can be dragged
//! onto a folder to move them.

mod edit;
mod menu;
mod nav;
mod ops;
mod render;
mod restore;
mod watch;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyBinding, ScrollStrategy, SharedString,
    UniformListScrollHandle, actions,
};

use crate::project::{DirEntry, read_dir_sorted};

pub use edit::EditKind;

actions!(
    file_tree,
    [
        SelectPrev,
        SelectNext,
        SelectFirst,
        SelectLast,
        Expand,
        Collapse,
        Open,
        /// Creates a file in the selected folder (typed inline).
        NewFile,
        /// Creates a folder in the selected folder (typed inline).
        NewFolder,
        /// Renames the selected file or folder (typed inline).
        Rename,
        /// Deletes the selected file or folder (to the trash).
        Delete,
        /// Copies the absolute path of the selection.
        CopyPath,
        /// Copies the project-relative path of the selection.
        CopyRelativePath,
        /// Shows the selection in the system file manager.
        RevealInFileManager,
        /// Opens a terminal in the selected folder (or the file's folder).
        OpenInTerminal,
        /// Opens the context menu for the selection.
        ShowContextMenu,
        /// Confirms the inline name being typed.
        ConfirmEdit,
        /// Cancels the inline name being typed, or closes the menu.
        Cancel,
        /// Highlights the previous context menu entry.
        MenuPrev,
        /// Highlights the next context menu entry.
        MenuNext,
        /// Runs the highlighted context menu entry.
        MenuConfirm,
    ]
);

/// Key context of the tree.
pub const CONTEXT: &str = "FileTree";
/// Key context of the inline name field.
const EDIT_CONTEXT: &str = "FileTreeEdit";
pub(crate) const ROW_HEIGHT: f32 = 24.;
pub(crate) const INDENT: f32 = 12.;
pub(crate) const LEFT_PAD: f32 = 8.;

/// Registers the tree's keyboard bindings.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    let edit = Some(EDIT_CONTEXT);
    let delete = if cfg!(target_os = "macos") {
        "cmd-backspace"
    } else {
        "delete"
    };
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("home", SelectFirst, ctx),
        KeyBinding::new("end", SelectLast, ctx),
        KeyBinding::new("right", Expand, ctx),
        KeyBinding::new("left", Collapse, ctx),
        KeyBinding::new("space", Open, ctx),
        KeyBinding::new("enter", Open, ctx),
        KeyBinding::new("f2", Rename, ctx),
        KeyBinding::new(delete, Delete, ctx),
        KeyBinding::new("secondary-alt-c", CopyPath, ctx),
        KeyBinding::new("secondary-alt-shift-c", CopyRelativePath, ctx),
        KeyBinding::new("shift-f10", ShowContextMenu, ctx),
        KeyBinding::new("menu", ShowContextMenu, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("enter", ConfirmEdit, edit),
        KeyBinding::new("escape", Cancel, edit),
    ]);
    // While the menu is open the tree's context also has `menu`; these are
    // added last so they win over the tree's own arrows and Enter.
    let menu = Some("FileTree && menu");
    cx.bind_keys([
        KeyBinding::new("up", MenuPrev, menu),
        KeyBinding::new("down", MenuNext, menu),
        KeyBinding::new("enter", MenuConfirm, menu),
        KeyBinding::new("escape", Cancel, menu),
    ]);
}

/// The `[workspace]` settings (defaults when none are installed).
fn workspace_settings(cx: &App) -> arcode_config::WorkspaceSettings {
    cx.try_global::<arcode_ui_kit::Settings>()
        .map(|s| s.0.workspace.clone())
        .unwrap_or_default()
}

/// Events emitted by the [`FileTree`].
#[derive(Clone, Debug, PartialEq)]
pub enum FileTreeEvent {
    /// A file was chosen (or just created).
    OpenFile(PathBuf),
    /// A file or folder was renamed or moved.
    Renamed {
        /// The old path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// A file or folder was deleted (or moved to the trash).
    Deleted(PathBuf),
    /// Open a terminal in this folder.
    OpenTerminal(PathBuf),
    /// Something to tell the user (an error).
    Message(String),
}

/// One visible row.
pub(crate) struct Row {
    pub(crate) path: PathBuf,
    pub(crate) name: SharedString,
    pub(crate) depth: usize,
    pub(crate) is_dir: bool,
    pub(crate) expanded: bool,
    /// The inline "new file / new folder" row (not on disk yet).
    pub(crate) placeholder: bool,
}

/// The file tree view of one project root.
pub struct FileTree {
    root: PathBuf,
    title: SharedString,
    children: HashMap<PathBuf, Vec<DirEntry>>,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected: Option<PathBuf>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    /// The inline name field, while creating or renaming.
    edit: Option<edit::InlineEdit>,
    /// The context menu, while open.
    menu: Option<menu::ContextMenu>,
    /// File-system watching of the root and the expanded folders.
    watch: Option<watch::TreeWatch>,
    /// Bounds of the row list in the last frame (to place the menu).
    list_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    /// Folders being expanded from a session, read in the background.
    restoring: Option<(Vec<PathBuf>, gpui::Task<()>)>,
}

impl EventEmitter<FileTreeEvent> for FileTree {}

impl Focusable for FileTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FileTree {
    /// Creates a tree for `root`, showing its direct children. Watches the
    /// shown folders when `workspace.watch_files` is on.
    pub fn new(root: PathBuf, title: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let mut tree = Self {
            root,
            title: title.into(),
            children: HashMap::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            edit: None,
            menu: None,
            watch: None,
            list_bounds: Default::default(),
            restoring: None,
        };
        tree.rebuild();
        if workspace_settings(cx).watch_files {
            tree.watch = watch::TreeWatch::start(cx);
            tree.sync_watches();
        }
        tree
    }

    /// The project root shown.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Re-reads every expanded folder from disk.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.children.clear();
        self.expanded.retain(|p| p.is_dir());
        self.rebuild();
        self.sync_watches();
        cx.notify();
    }

    /// Collapses every folder.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.rebuild();
        self.sync_watches();
        cx.notify();
    }

    /// Clears the selection (e.g. when the last tab closed).
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// The selected path, if any.
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    /// Expands the folders containing `path`, selects it and scrolls to it.
    pub fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            // Outside this project: nothing in the tree matches the file,
            // so no stale entry should stay selected.
            self.clear_selection(cx);
            return;
        };
        let mut dir = self.root.clone();
        let mut parts = rel.components().peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                dir.push(part);
                self.expanded.insert(dir.clone());
            }
        }
        self.selected = Some(path.to_path_buf());
        self.rebuild();
        self.sync_watches();
        if let Some(ix) = self.rows.iter().position(|r| r.path == path) {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn entries(&mut self, dir: &Path) -> &[DirEntry] {
        self.children
            .entry(dir.to_path_buf())
            .or_insert_with(|| read_dir_sorted(dir))
    }

    /// Flattens the expanded tree into visible rows.
    ///
    /// Only the root and expanded folders keep a cached listing: a
    /// collapsed folder is not watched, so its listing would go stale and
    /// is read again when it is next expanded.
    pub(crate) fn rebuild(&mut self) {
        let (root, expanded) = (&self.root, &self.expanded);
        self.children
            .retain(|dir, _| dir == root || expanded.contains(dir));
        let mut rows = Vec::new();
        let new_in = self.edit.as_ref().and_then(|e| e.new_entry_dir()).cloned();
        let root = self.root.clone();
        if new_in.as_ref() == Some(&root) {
            rows.push(self.placeholder_row(&root, 0));
        }
        // Depth-first walk; children are pushed in reverse to keep order.
        let mut pending: Vec<(DirEntry, usize)> = self
            .entries(&root)
            .iter()
            .rev()
            .map(|e| (e.clone(), 0))
            .collect();
        while let Some((entry, depth)) = pending.pop() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            if expanded {
                let children = self.entries(&entry.path);
                pending.extend(children.iter().rev().map(|c| (c.clone(), depth + 1)));
            }
            let placeholder_here = expanded && new_in.as_ref() == Some(&entry.path);
            let path = entry.path.clone();
            rows.push(Row {
                name: entry.name.into(),
                path: entry.path,
                depth,
                is_dir: entry.is_dir,
                expanded,
                placeholder: false,
            });
            if placeholder_here {
                rows.push(self.placeholder_row(&path, depth + 1));
            }
        }
        self.rows = rows;
    }

    fn placeholder_row(&self, dir: &Path, depth: usize) -> Row {
        let is_dir = matches!(
            self.edit.as_ref().map(|e| &e.kind),
            Some(EditKind::NewFolder(_))
        );
        Row {
            path: dir.join("\u{0}new"),
            name: SharedString::default(),
            depth,
            is_dir,
            expanded: false,
            placeholder: true,
        }
    }

    fn toggle(&mut self, path: &Path) {
        if self.expanded.remove(path) {
            // Folders expanded inside it stay expanded (and, when watching,
            // watched), but are read afresh when shown again.
            if self.watch.is_none() {
                self.children.retain(|dir, _| !dir.starts_with(path));
            }
        } else {
            self.expanded.insert(path.to_path_buf());
        }
        self.rebuild();
        self.sync_watches();
    }

    pub(crate) fn click(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix).filter(|r| !r.placeholder) else {
            return;
        };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        self.selected = Some(path.clone());
        if is_dir {
            self.toggle(&path);
        } else {
            cx.emit(FileTreeEvent::OpenFile(path));
        }
        cx.notify();
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_ref()?;
        self.rows.iter().position(|r| &r.path == sel)
    }
}

#[cfg(test)]
mod review_tree;
#[cfg(test)]
mod tests;
