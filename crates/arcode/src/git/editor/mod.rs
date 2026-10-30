//! Git features inside editors: change bars in the gutter with a hunk
//! popover, inline blame and the blame column, and merge conflict regions.
//!
//! [`GitEditors`] tracks every open editor of a workspace. Work is
//! debounced after edits and runs on the background executor; results are
//! applied with the editor extension points under the set keys below.

mod blame;
mod conflicts;
mod hunks;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcode_editor::{Editor, EditorEvent, GutterLane};
use arcode_git::{BlameLine, Conflict, GutterHunk, Repo};
use gpui::{Context, Entity, EntityId, EventEmitter, Subscription, Task, WeakEntity};

use super::store::{GitEvent, GitStore};

pub use conflicts::ConflictAction;
pub use hunks::HunkAction;

/// Set key of the change bars and the hunk popover.
pub const GUTTER_SET: &str = "git";
/// Set key of the inline blame at the end of the cursor line.
pub const BLAME_SET: &str = "git-blame";
/// Set key of the blame column.
pub const BLAME_COLUMN_SET: &str = "git-blame-column";
/// Set key of merge conflict highlights and actions.
pub const CONFLICT_SET: &str = "git-conflict";

/// Events for the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitEditorsEvent {
    /// Open a read-only diff tab of `old` against `new`.
    OpenDiff {
        /// Tab title.
        title: String,
        /// File whose language colors the diff, if any.
        path: Option<PathBuf>,
        /// The old text.
        old: String,
        /// The new text.
        new: String,
    },
    /// A message for the user.
    Message(String),
    /// The conflict count of an editor changed (status bar).
    ConflictsChanged,
}

/// Git state of one editor.
pub(crate) struct EditorGit {
    pub editor: WeakEntity<Editor>,
    pub path: PathBuf,
    pub repo: Option<Repo>,
    pub hunks: Vec<GutterHunk>,
    pub hunks_version: Option<u64>,
    pub hunk_task: Option<Task<()>>,
    pub blame: Option<(u64, Arc<Vec<BlameLine>>)>,
    pub blame_task: Option<Task<()>>,
    pub blame_shown: Option<usize>,
    pub column_task: Option<Task<()>>,
    pub conflicts: Vec<Conflict>,
    pub _sub: Subscription,
}

impl EditorGit {
    /// The path relative to its repository.
    pub fn rel_path(&self) -> Option<PathBuf> {
        let repo = self.repo.as_ref()?;
        self.path
            .strip_prefix(&repo.workdir)
            .ok()
            .map(Path::to_path_buf)
    }
}

/// Git decorations for every editor of a workspace.
pub struct GitEditors {
    pub(crate) store: Entity<GitStore>,
    pub(crate) editors: HashMap<EntityId, EditorGit>,
    pub(crate) active: Option<EntityId>,
    pub(crate) blame_column: bool,
    _store_sub: Subscription,
}

impl EventEmitter<GitEditorsEvent> for GitEditors {}

impl GitEditors {
    /// Creates the tracker; it follows `store` for index and HEAD changes.
    pub fn new(store: Entity<GitStore>, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(&store, |this, _, event: &GitEvent, cx| {
            if *event == GitEvent::IndexChanged || *event == GitEvent::ReposChanged {
                this.refresh_all(cx);
            }
        });
        Self {
            store,
            editors: HashMap::new(),
            active: None,
            blame_column: false,
            _store_sub: sub,
        }
    }

    /// Starts tracking `editor` (a file editor; others are ignored).
    pub fn register(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let Some(path) = editor.read(cx).path().map(Path::to_path_buf) else {
            return;
        };
        if editor.read(cx).is_read_only() {
            return;
        }
        let repo = self.store.read(cx).repo_for_path(&path);
        let sub = cx.subscribe(editor, Self::on_editor_event);
        let id = editor.entity_id();
        self.editors.insert(
            id,
            EditorGit {
                editor: editor.downgrade(),
                path,
                repo,
                hunks: Vec::new(),
                hunks_version: None,
                hunk_task: None,
                blame: None,
                blame_task: None,
                blame_shown: None,
                column_task: None,
                conflicts: Vec::new(),
                _sub: sub,
            },
        );
        cx.observe_release(editor, move |this, _, cx| {
            this.editors.remove(&id);
            cx.emit(GitEditorsEvent::ConflictsChanged);
        })
        .detach();
        self.schedule_analysis(id, false, cx);
        if self.blame_column {
            self.schedule_blame_column(id, false, cx);
        }
    }

    /// Sets the editor whose cursor line gets inline blame.
    pub fn set_active(&mut self, editor: Option<&Entity<Editor>>, cx: &mut Context<Self>) {
        let id = editor.map(Entity::entity_id);
        if self.active == id {
            return;
        }
        if let Some(old) = self.active.take() {
            self.clear_inline_blame(old, cx);
        }
        self.active = id;
        if let Some(id) = id {
            self.schedule_inline_blame(id, cx);
        }
        cx.emit(GitEditorsEvent::ConflictsChanged);
    }

    /// Whether the blame column is shown.
    pub fn blame_column(&self) -> bool {
        self.blame_column
    }

    /// The conflict regions of `editor`.
    pub fn conflicts(&self, editor: EntityId) -> &[Conflict] {
        self.editors
            .get(&editor)
            .map_or(&[], |e| e.conflicts.as_slice())
    }

    /// The gutter hunks of `editor`.
    pub fn hunks(&self, editor: EntityId) -> &[GutterHunk] {
        self.editors
            .get(&editor)
            .map_or(&[], |e| e.hunks.as_slice())
    }

    /// Recomputes everything for every editor (index or HEAD changed).
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<EntityId> = self.editors.keys().copied().collect();
        for id in ids {
            let path = self.editors[&id].path.clone();
            let repo = self.store.read(cx).repo_for_path(&path);
            if let Some(state) = self.editors.get_mut(&id) {
                state.repo = repo;
                state.blame = None;
            }
            self.schedule_analysis(id, false, cx);
            if self.blame_column {
                self.schedule_blame_column(id, false, cx);
            }
        }
        if let Some(id) = self.active {
            self.clear_inline_blame(id, cx);
            self.schedule_inline_blame(id, cx);
        }
    }

    fn on_editor_event(
        &mut self,
        editor: Entity<Editor>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        match event {
            EditorEvent::Edited { .. } => {
                editor.update(cx, |e, cx| {
                    e.hide_popover(GUTTER_SET, cx);
                });
                self.clear_inline_blame(id, cx);
                self.schedule_analysis(id, true, cx);
                if self.blame_column {
                    self.schedule_blame_column(id, true, cx);
                }
            }
            EditorEvent::Saved => {
                if let Some(state) = self.editors.get_mut(&id) {
                    state.blame = None;
                }
                self.store.update(cx, |s, cx| s.refresh(cx));
            }
            EditorEvent::SelectionsChanged => {
                if self.active == Some(id) {
                    self.schedule_inline_blame(id, cx);
                }
            }
            EditorEvent::GutterClicked {
                lane: GutterLane::Changes,
                tag: Some(tag),
                ..
            } => self.toggle_hunk_popover(&editor, *tag as usize, cx),
            EditorEvent::CodeLensClicked {
                set,
                line,
                id: lens,
            } if set == CONFLICT_SET => {
                self.conflict_lens_clicked(&editor, *line, *lens, cx);
            }
            _ => {}
        }
    }
}
