//! The inline name field: typing the name of a new file or folder, or the
//! new name of an entry, right in the tree.

use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Entity, Focusable, SharedString, Subscription, Window};

use super::{FileTree, FileTreeEvent};
use crate::text_field::{TextChanged, TextField};

/// What the inline field creates or changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// A new file in this folder.
    NewFile(PathBuf),
    /// A new folder in this folder.
    NewFolder(PathBuf),
    /// A new name for this file or folder.
    Rename(PathBuf),
}

/// The field and its state.
pub(crate) struct InlineEdit {
    pub(crate) kind: EditKind,
    pub(crate) input: Entity<TextField>,
    pub(crate) error: Option<SharedString>,
    /// The text the error is about (a change of the text clears it).
    error_for: String,
    _subs: Vec<Subscription>,
}

impl InlineEdit {
    /// The folder that shows a placeholder row (new file / folder).
    pub(crate) fn new_entry_dir(&self) -> Option<&PathBuf> {
        match &self.kind {
            EditKind::NewFile(dir) | EditKind::NewFolder(dir) => Some(dir),
            EditKind::Rename(_) => None,
        }
    }
}

impl FileTree {
    /// The inline edit in progress, if any.
    pub fn editing(&self) -> Option<&EditKind> {
        self.edit.as_ref().map(|e| &e.kind)
    }

    /// The text typed in the inline field.
    pub fn edit_text(&self, cx: &gpui::App) -> Option<String> {
        self.edit
            .as_ref()
            .map(|e| e.input.read(cx).text().to_owned())
    }

    /// The error shown under the inline field.
    pub fn edit_error(&self) -> Option<&str> {
        self.edit
            .as_ref()
            .and_then(|e| e.error.as_ref().map(|e| e.as_ref()))
    }

    /// Replaces the text of the inline field (tests, scripting).
    pub fn set_edit_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(edit) = &self.edit {
            edit.input.update(cx, |f, cx| f.set_text(text, cx));
        }
    }

    /// Starts typing a name inline. A new entry's folder is expanded and
    /// the placeholder row shown at its top; a rename preselects the name
    /// without its extension.
    pub fn start_edit(&mut self, kind: EditKind, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let initial = match &kind {
            EditKind::Rename(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let placeholder = match &kind {
            EditKind::NewFile(_) => "File name",
            EditKind::NewFolder(_) => "Folder name",
            EditKind::Rename(_) => "New name",
        };
        let input = cx.new(|cx| {
            let mut field = TextField::new(placeholder, cx);
            field.set_text(&initial, cx);
            field.select_all_text(cx);
            field
        });
        let changed = cx.subscribe(&input, |tree, input, _: &TextChanged, cx| {
            // Change events arrive after the update that typed the text,
            // so an error about the current text is kept.
            if let Some(edit) = &mut tree.edit
                && input.read(cx).text() != edit.error_for
                && edit.error.take().is_some()
            {
                cx.notify();
            }
        });
        let blur = cx.on_blur(&input.focus_handle(cx), window, |tree, window, cx| {
            // Clicking elsewhere keeps a valid name and drops the rest.
            tree.confirm_edit_inner(true, window, cx);
        });
        if let EditKind::NewFile(dir) | EditKind::NewFolder(dir) = &kind
            && *dir != self.root
        {
            self.expanded.insert(dir.clone());
        }
        window.focus(&input.focus_handle(cx));
        self.edit = Some(InlineEdit {
            kind,
            input,
            error: None,
            error_for: String::new(),
            _subs: vec![changed, blur],
        });
        self.rebuild();
        self.sync_watches();
        if let Some(ix) = self.rows.iter().position(|r| {
            r.placeholder || matches!(self.editing(), Some(EditKind::Rename(p)) if *p == r.path)
        }) {
            self.scroll.scroll_to_item(ix, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Cancels the inline edit and gives the focus back to the tree.
    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.take().is_some() {
            self.rebuild();
            window.focus(&self.focus_handle);
            cx.notify();
        }
    }

    pub(super) fn confirm_edit(
        &mut self,
        _: &super::ConfirmEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_edit_inner(false, window, cx);
    }

    /// Applies the typed name. With `on_blur` an invalid or unchanged
    /// name just ends the edit; otherwise the error is shown and the field
    /// stays open.
    pub(super) fn confirm_edit_inner(
        &mut self,
        on_blur: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = &self.edit else { return };
        let name = edit.input.read(cx).text().to_owned();
        let kind = edit.kind.clone();
        let unchanged = match &kind {
            EditKind::Rename(path) => path.file_name().is_some_and(|n| n == name.as_str()),
            _ => name.is_empty(),
        };
        if unchanged {
            self.finish_edit(on_blur, window, cx);
            return;
        }
        let result = match &kind {
            EditKind::NewFile(dir) => arcode_project::files::create_file(dir, &name),
            EditKind::NewFolder(dir) => arcode_project::files::create_dir(dir, &name),
            EditKind::Rename(path) => {
                self.release_watches_under(path);
                arcode_project::files::rename(path, &name)
            }
        };
        if result.is_err() {
            self.sync_watches();
        }
        match result {
            Ok(path) => {
                self.finish_edit(on_blur, window, cx);
                self.applied(&kind, &path, cx);
            }
            Err(err) if on_blur => {
                self.finish_edit(true, window, cx);
                cx.emit(FileTreeEvent::Message(format!(
                    "Cannot use “{name}”: {err}"
                )));
            }
            Err(err) => {
                if let Some(edit) = &mut self.edit {
                    edit.error = Some(err.to_string().into());
                    edit.error_for = name;
                }
                cx.notify();
            }
        }
    }

    fn finish_edit(&mut self, on_blur: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.edit = None;
        self.rebuild();
        if !on_blur {
            window.focus(&self.focus_handle);
        }
        cx.notify();
    }

    /// Updates the tree after a create or rename and tells the workspace.
    fn applied(&mut self, kind: &EditKind, path: &Path, cx: &mut Context<Self>) {
        let parent = path.parent().map(Path::to_path_buf);
        if let Some(parent) = &parent {
            self.children.remove(parent);
        }
        match kind {
            EditKind::Rename(from) => {
                self.renamed(from, path);
                cx.emit(FileTreeEvent::Renamed {
                    from: from.clone(),
                    to: path.to_path_buf(),
                });
            }
            EditKind::NewFile(_) => cx.emit(FileTreeEvent::OpenFile(path.to_path_buf())),
            EditKind::NewFolder(_) => {}
        }
        self.selected = Some(path.to_path_buf());
        self.rebuild();
        self.sync_watches();
        if let Some(ix) = self.selected_index() {
            self.scroll.scroll_to_item(ix, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Moves cached state (expanded folders, children) from `from` to `to`.
    pub(super) fn renamed(&mut self, from: &Path, to: &Path) {
        use arcode_project::files::renamed_path;
        self.expanded = self
            .expanded
            .drain()
            .map(|p| renamed_path(&p, from, to).unwrap_or(p))
            .collect();
        self.children.retain(|dir, _| !dir.starts_with(from));
        for dir in [from.parent(), to.parent()].into_iter().flatten() {
            self.children.remove(dir);
        }
        if let Some(sel) = &self.selected
            && let Some(new) = renamed_path(sel, from, to)
        {
            self.selected = Some(new);
        }
    }
}
