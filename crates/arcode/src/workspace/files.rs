//! The file tree's side of the workspace: opening files it picks, keeping
//! editors in step with renames and deletes, terminals in a folder, and
//! the Explorer commands when the tree is not focused (they act on the
//! active file).

use std::path::{Path, PathBuf};

use arcode_lsp::FileChange;
use gpui::{AppContext, ClipboardItem, Context, Entity, Focusable, Window};

use super::Workspace;
use crate::actions::RevealActiveFile;
use crate::activity_bar::Activity;
use crate::project::relative_display;
use crate::sidebar::file_tree::{
    CopyPath, CopyRelativePath, EditKind, FileTree, FileTreeEvent, NewFile, NewFolder,
    OpenInTerminal, RevealInFileManager,
};

impl Workspace {
    /// Creates the tree of `root` and follows its events.
    pub(super) fn make_file_tree(
        &mut self,
        root: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FileTree> {
        let (git, lsp) = (self.git.store.clone(), self.lsp.clone());
        let tree = cx.new(|cx| {
            let mut tree = FileTree::new(root, name, cx);
            // Git letters and error counts on the rows.
            tree.follow_sources(git, lsp, cx);
            tree
        });
        let sub =cx.subscribe_in(&tree, window, |ws, _, event: &FileTreeEvent, window, cx| {
            ws.on_tree_event(event, window, cx)
        });
        self.subs.set("file_tree", sub);
        tree
    }

    pub(super) fn on_tree_event(
        &mut self,
        event: &FileTreeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FileTreeEvent::OpenFile(path) => self.open_file(path, window, cx),
            FileTreeEvent::Renamed { from, to } => self.paths_renamed(from, to, cx),
            FileTreeEvent::Deleted(path) => self.paths_deleted(path, window, cx),
            FileTreeEvent::OpenTerminal(dir) => {
                self.bottom_panel_visible = true;
                self.bottom_panel.update(cx, |panel, cx| {
                    panel.new_terminal_in(dir.clone(), window, cx)
                });
                cx.notify();
            }
            FileTreeEvent::Message(text) => self.show_toast(text.clone(), cx),
        }
    }

    /// Points editors of renamed files (or files inside a renamed folder)
    /// at their new path.
    fn paths_renamed(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        for editor in self.editors(cx) {
            let new = editor
                .read(cx)
                .path()
                .and_then(|p| arcode_project::files::renamed_path(p, from, to));
            if let Some(new) = new {
                editor.update(cx, |e, cx| e.set_path(new, cx));
                self.reapply_association(&editor, cx);
            }
        }
        for pane in self.panes.clone() {
            pane.update(cx, |_, cx| cx.notify());
        }
        self.rename_file_states(from, to);
        self.debug
            .debugger
            .update(cx, |d, cx| d.path_renamed(from, to, cx));
        self.schedule_session_save(cx);
        self.refresh_status(cx);
        self.report_file_change(FileChange::Renamed(from.into(), to.into()), cx);
    }

    /// Tells the language servers about a file change made in the tree,
    /// once the editors' path events (which reopen their documents under
    /// the new paths) were delivered.
    fn report_file_change(&self, change: FileChange, cx: &mut Context<Self>) {
        let lsp = self.lsp.clone();
        cx.defer(move |cx| lsp.read(cx).files_changed(&[change]));
    }

    /// Closes the tabs of deleted files that have no unsaved changes (the
    /// others stay open so nothing typed is lost).
    fn paths_deleted(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        for pane in self.panes.clone() {
            loop {
                let ix = pane.read(cx).items().iter().position(|e| {
                    let e = e.read(cx);
                    !e.is_dirty() && e.path().is_some_and(|p| p.starts_with(path))
                });
                let Some(ix) = ix else { break };
                pane.update(cx, |pane, cx| pane.close(ix, window, cx));
            }
        }
        self.forget_file_states(path);
        self.schedule_session_save(cx);
        self.report_file_change(FileChange::Deleted(path.into()), cx);
    }

    /// Registers the Explorer commands handled by the workspace.
    pub(super) fn register_file_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|ws, _: &NewFile, window, cx| ws.start_tree_edit(false, window, cx)),
        )
        .on_action(
            cx.listener(|ws, _: &NewFolder, window, cx| ws.start_tree_edit(true, window, cx)),
        )
        .on_action(
            cx.listener(|ws, _: &RevealActiveFile, window, cx| ws.reveal_active_file(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &CopyPath, _, cx| ws.copy_active_path(false, cx)))
        .on_action(cx.listener(|ws, _: &CopyRelativePath, _, cx| ws.copy_active_path(true, cx)))
        .on_action(cx.listener(|ws, _: &RevealInFileManager, _, cx| {
            if let Some(path) = ws.active_file_path(cx) {
                let task = cx.background_spawn(async move {
                    arcode_project::files::reveal_in_file_manager(&path)
                });
                cx.spawn(async move |this, cx| {
                    if let Err(err) = task.await {
                        this.update(cx, |ws, cx| {
                            ws.show_toast(format!("Cannot open the file manager: {err}"), cx)
                        })
                        .ok();
                    }
                })
                .detach();
            }
        }))
        .on_action(cx.listener(|ws, _: &OpenInTerminal, window, cx| {
            let dir = ws
                .active_file_path(cx)
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .or_else(|| ws.project_root().map(Path::to_path_buf));
            if let Some(dir) = dir {
                ws.on_tree_event(&FileTreeEvent::OpenTerminal(dir), window, cx);
            }
        }))
    }

    fn active_file_path(&self, cx: &gpui::App) -> Option<PathBuf> {
        self.active_editor(cx)
            .and_then(|e| e.read(cx).path().map(Path::to_path_buf))
    }

    fn copy_active_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(path) = self.active_file_path(cx) else {
            return;
        };
        let text = match (relative, self.project_root()) {
            (true, Some(root)) => relative_display(root, &path),
            _ => path.display().to_string(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Shows the Explorer and focuses the tree.
    fn show_tree(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<FileTree>> {
        let tree = self.file_tree.clone()?;
        self.activity = Activity::Files;
        self.sidebar_visible = true;
        window.focus(&tree.focus_handle(cx));
        cx.notify();
        Some(tree)
    }

    /// New File / New Folder from outside the tree: next to the active
    /// file, else in the project root.
    fn start_tree_edit(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.show_toast("Open a folder first", cx);
            return;
        };
        let dir = self
            .active_file_path(cx)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|d| d.starts_with(&root))
            .unwrap_or(root);
        let Some(tree) = self.show_tree(window, cx) else {
            return;
        };
        let kind = if folder {
            EditKind::NewFolder(dir)
        } else {
            EditKind::NewFile(dir)
        };
        tree.update(cx, |tree, cx| tree.start_edit(kind, window, cx));
    }

    fn reveal_active_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.active_file_path(cx);
        if let Some(tree) = self.show_tree(window, cx)
            && let Some(path) = path
        {
            tree.update(cx, |tree, cx| tree.reveal(&path, cx));
        }
    }

    /// Number of editors with unsaved changes.
    pub fn dirty_count(&self, cx: &gpui::App) -> usize {
        self.editors(cx)
            .iter()
            .filter(|e| e.read(cx).is_dirty())
            .count()
    }
}
