//! Tree commands: new file / folder, rename, delete, copy paths, reveal in
//! the file manager, open a terminal, and moving rows by drag and drop.

use std::path::{Path, PathBuf};

use gpui::{AppContext, ClipboardItem, Context, PromptLevel, Window};

use super::*;
use crate::project::relative_display;

impl FileTree {
    /// The entry a command applies to: the context menu's target, else the
    /// selection, else the root. Returns the path and whether it is a
    /// folder.
    pub(crate) fn command_target(&self) -> (PathBuf, bool) {
        let path = self
            .menu
            .as_ref()
            .map(|m| m.target.clone())
            .or_else(|| self.selected.clone())
            .unwrap_or_else(|| self.root.clone());
        let is_dir = path.is_dir();
        (path, is_dir)
    }

    /// The folder a new entry goes into for the current target.
    fn target_dir(&self) -> PathBuf {
        let (path, is_dir) = self.command_target();
        if is_dir {
            path
        } else {
            path.parent()
                .filter(|p| p.starts_with(&self.root))
                .map_or_else(|| self.root.clone(), Path::to_path_buf)
        }
    }

    pub(super) fn new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.target_dir();
        self.start_edit(EditKind::NewFile(dir), window, cx);
    }

    pub(super) fn new_folder(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = self.target_dir();
        self.start_edit(EditKind::NewFolder(dir), window, cx);
    }

    pub(super) fn rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        let (path, _) = self.command_target();
        if path == self.root {
            return;
        }
        self.selected = Some(path.clone());
        self.start_edit(EditKind::Rename(path), window, cx);
    }

    pub(super) fn copy_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        let (path, _) = self.command_target();
        self.menu = None;
        cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
        cx.notify();
    }

    pub(super) fn copy_relative_path(
        &mut self,
        _: &CopyRelativePath,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, _) = self.command_target();
        self.menu = None;
        let rel = relative_display(&self.root, &path);
        let rel = if rel.is_empty() { ".".to_owned() } else { rel };
        cx.write_to_clipboard(ClipboardItem::new_string(rel));
        cx.notify();
    }

    pub(super) fn reveal_in_file_manager(
        &mut self,
        _: &RevealInFileManager,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, _) = self.command_target();
        self.menu = None;
        cx.notify();
        let task = cx.background_spawn({
            let path = path.clone();
            async move { arcode_project::files::reveal_in_file_manager(&path) }
        });
        cx.spawn(async move |this, cx| {
            if let Err(err) = task.await {
                this.update(cx, |_, cx| {
                    cx.emit(FileTreeEvent::Message(format!(
                        "Cannot show {} in the file manager: {err}",
                        path.display()
                    )))
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn open_in_terminal(
        &mut self,
        _: &OpenInTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = self.target_dir();
        self.menu = None;
        cx.emit(FileTreeEvent::OpenTerminal(dir));
        cx.notify();
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        let (path, is_dir) = self.command_target();
        self.menu = None;
        cx.notify();
        if path == self.root {
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !workspace_settings(cx).confirm_delete {
            self.trash(path, window, cx);
            return;
        }
        let what = if is_dir { "folder" } else { "file" };
        let detail = if is_dir {
            "The folder and everything in it go to the trash."
        } else {
            "You can restore it from the trash."
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete the {what} “{name}”?"),
            Some(detail),
            &["Move to Trash", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |tree, window, cx| tree.trash(path, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Moves `path` to the trash in the background; when there is no
    /// trash, asks whether to delete it permanently.
    fn trash(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.release_watches_under(&path);
        let task = cx.background_spawn({
            let path = path.clone();
            async move { arcode_project::files::move_to_trash(&path) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |tree, window, cx| match result {
                Ok(()) => tree.deleted(&path, cx),
                Err(arcode_project::files::TrashError::Unavailable(why)) => {
                    tree.sync_watches();
                    tree.confirm_permanent_delete(path, why, window, cx)
                }
                Err(arcode_project::files::TrashError::Io(err)) => {
                    tree.sync_watches();
                    cx.emit(FileTreeEvent::Message(format!(
                        "Cannot delete {}: {err}",
                        path.display()
                    )))
                }
            })
            .ok();
        })
        .detach();
    }

    fn confirm_permanent_delete(
        &mut self,
        path: PathBuf,
        why: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let detail = format!("It cannot be moved to the trash ({why}). This cannot be undone.");
        let answer = window.prompt(
            PromptLevel::Critical,
            &format!("Delete “{name}” permanently?"),
            Some(&detail),
            &["Delete Permanently", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |tree, _| tree.release_watches_under(&path))
                .ok();
            let target = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { arcode_project::files::delete_permanently(&target) })
                .await;
            this.update(cx, |tree, cx| match result {
                Ok(()) => tree.deleted(&path, cx),
                Err(err) => {
                    tree.sync_watches();
                    cx.emit(FileTreeEvent::Message(format!(
                        "Cannot delete {}: {err}",
                        path.display()
                    )))
                }
            })
            .ok();
        })
        .detach();
    }

    /// Updates the tree after `path` was deleted and tells the workspace.
    pub(crate) fn deleted(&mut self, path: &Path, cx: &mut Context<Self>) {
        let next = self.selected_index();
        self.expanded.retain(|p| !p.starts_with(path));
        self.children.retain(|dir, _| !dir.starts_with(path));
        if let Some(parent) = path.parent() {
            self.children.remove(parent);
        }
        if self
            .selected
            .as_deref()
            .is_some_and(|s| s.starts_with(path))
        {
            self.selected = None;
        }
        self.rebuild();
        self.sync_watches();
        if self.selected.is_none()
            && let Some(ix) = next
        {
            let ix = ix.min(self.rows.len().saturating_sub(1));
            self.selected = self.rows.get(ix).map(|r| r.path.clone());
        }
        cx.emit(FileTreeEvent::Deleted(path.to_path_buf()));
        cx.notify();
    }

    /// Moves `from` into the folder `dir` (drag and drop).
    pub fn move_entry(&mut self, from: &Path, dir: &Path, cx: &mut Context<Self>) {
        if from.parent() == Some(dir) || from == dir {
            cx.notify();
            return;
        }
        self.release_watches_under(from);
        match arcode_project::files::move_into(from, dir) {
            Ok(to) => {
                self.renamed(from, &to);
                self.children.remove(dir);
                if dir != self.root {
                    self.expanded.insert(dir.to_path_buf());
                }
                self.selected = Some(to.clone());
                self.rebuild();
                self.sync_watches();
                cx.emit(FileTreeEvent::Renamed {
                    from: from.to_path_buf(),
                    to,
                });
            }
            Err(err) => {
                self.sync_watches();
                cx.emit(FileTreeEvent::Message(format!(
                    "Cannot move {}: {err}",
                    from.display()
                )))
            }
        }
        cx.notify();
    }
}
