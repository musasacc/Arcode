//! Expanding the folders saved in a session. Their listings are read on
//! the background executor (a session may hold hundreds of folders), then
//! shown in one go.

use std::path::PathBuf;

use gpui::{AppContext, Context};

use super::FileTree;
use crate::project::{DirEntry, read_dir_sorted};

impl FileTree {
    /// The expanded folders, sorted (for the session), including those
    /// still being restored.
    pub fn expanded_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        if let Some((pending, _)) = &self.restoring {
            dirs.extend(pending.iter().cloned());
        }
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Expands `dirs` (those inside the root that still exist), in
    /// addition to the folders already open. The folders are read in the
    /// background; the tree shows them once all are read.
    pub fn set_expanded_dirs(&mut self, dirs: &[PathBuf], cx: &mut Context<Self>) {
        let dirs: Vec<PathBuf> = dirs
            .iter()
            .filter(|d| d.starts_with(&self.root))
            .cloned()
            .collect();
        if dirs.is_empty() {
            self.restoring = None;
            return;
        }
        let to_read = dirs.clone();
        let read = cx.background_spawn(async move {
            to_read
                .into_iter()
                .filter(|d| d.is_dir())
                .map(|d| {
                    let listing = read_dir_sorted(&d);
                    (d, listing)
                })
                .collect::<Vec<_>>()
        });
        let task = cx.spawn(async move |this, cx| {
            let listings = read.await;
            this.update(cx, |tree, cx| tree.finish_restore(listings, cx))
                .ok();
        });
        self.restoring = Some((dirs, task));
    }

    fn finish_restore(&mut self, listings: Vec<(PathBuf, Vec<DirEntry>)>, cx: &mut Context<Self>) {
        self.restoring = None;
        for (dir, entries) in listings {
            self.expanded.insert(dir.clone());
            self.children.insert(dir, entries);
        }
        self.rebuild();
        self.sync_watches();
        if let Some(ix) = self.selected_index() {
            self.scroll.scroll_to_item(ix, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }
}
