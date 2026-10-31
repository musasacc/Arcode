//! Open files with unsaved changes in the Search view: their matches come
//! from the editor's text, not from the file on disk. A file listed from
//! disk is swapped for its buffer's matches as it streams in; once the
//! disk search is done, unsaved buffers it did not list (their disk text
//! has no match, the buffer has) are searched too.

use std::path::{Path, PathBuf};

use arcode_buffer::Rope;
use arcode_editor::Editor;
use arcode_search::{FileMatches, SearchQuery, search_text};
use gpui::{App, AppContext, AsyncApp, Entity, WeakEntity};

use super::ProjectSearch;

/// An unsaved buffer to search: absolute path, path relative to the root,
/// and a snapshot of its text.
pub(super) type Snapshot = (PathBuf, PathBuf, Rope);

impl ProjectSearch {
    /// `file` as found on disk, or, when it is open with unsaved changes,
    /// the matches of the editor's text instead (`None` if it has none).
    pub(super) fn prefer_buffer(
        &self,
        file: FileMatches,
        query: &SearchQuery,
        cx: &App,
    ) -> Option<FileMatches> {
        let Some(editor) = self
            .open_editor(&file.path, cx)
            .filter(|e| e.read(cx).is_dirty())
        else {
            return Some(file);
        };
        let text = editor.read(cx).buffer().text().to_string();
        let matches = search_text(&text, query).ok()?;
        (!matches.is_empty()).then_some(FileMatches { matches, ..file })
    }

    pub(super) fn open_editor(&self, path: &Path, cx: &App) -> Option<Entity<Editor>> {
        let editors = self.editors.as_ref()?;
        editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(path))
    }

    /// Snapshots of the unsaved open buffers under the root that the disk
    /// search did not list but would have visited (globs, hidden files).
    pub(super) fn unlisted_unsaved(&self, query: &SearchQuery, cx: &App) -> Vec<Snapshot> {
        let (Some(root), Some(editors)) = (&self.root, &self.editors) else {
            return Vec::new();
        };
        editors(cx)
            .into_iter()
            .filter_map(|editor| {
                let e = editor.read(cx);
                let path = e.path().filter(|_| e.is_dirty())?.to_path_buf();
                if self.results.files().iter().any(|f| f.path == path)
                    || !query.includes_path(root, &path)
                {
                    return None;
                }
                let relative = path.strip_prefix(root).ok()?.to_path_buf();
                Some((path, relative, e.buffer().text().clone()))
            })
            .collect()
    }
}

impl ProjectSearch {
    /// Unsaved open buffers under the root, not in `listed`, that the
    /// search would visit, with their match counts (those with matches).
    pub(super) fn unsaved_only_counts(
        &self,
        listed: &[PathBuf],
        query: &SearchQuery,
        cx: &App,
    ) -> Vec<(PathBuf, usize)> {
        let (Some(root), Some(editors)) = (&self.root, &self.editors) else {
            return Vec::new();
        };
        editors(cx)
            .into_iter()
            .filter_map(|editor| {
                let e = editor.read(cx);
                let path = e.path().filter(|_| e.is_dirty())?.to_path_buf();
                if listed.contains(&path) || !query.includes_path(root, &path) {
                    return None;
                }
                let n = super::replace::count_in_editor(&editor, query, cx);
                (n > 0).then_some((path, n))
            })
            .collect()
    }
}

/// After the disk search: searches the unsaved buffers it did not list (in
/// the background) and adds those that match. `None` when the view is gone.
pub(super) async fn add_unsaved(
    this: &WeakEntity<ProjectSearch>,
    query: SearchQuery,
    cx: &mut AsyncApp,
) -> Option<()> {
    let snapshots = this
        .update(cx, |view, cx| view.unlisted_unsaved(&query, cx))
        .ok()?;
    if snapshots.is_empty() {
        return Some(());
    }
    let found = cx
        .background_spawn(async move { search_snapshots(snapshots, &query) })
        .await;
    this.update(cx, |view, _| {
        for file in found {
            view.results.insert(file);
        }
        view.rebuild_rows();
    })
    .ok()
}

/// Searches buffer snapshots (on a background thread).
fn search_snapshots(snapshots: Vec<Snapshot>, query: &SearchQuery) -> Vec<FileMatches> {
    snapshots
        .into_iter()
        .filter_map(|(path, relative, text)| {
            let matches = search_text(&text.to_string(), query).ok()?;
            (!matches.is_empty()).then_some(FileMatches {
                path,
                relative,
                matches,
            })
        })
        .collect()
}
