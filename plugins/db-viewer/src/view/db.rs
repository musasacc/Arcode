//! The opener view of one database file (§10 "Opener / table tab"): the
//! schema tree on the left, **Data** and **Query** tabs on the right.
//!
//! This file holds the state and the operations that change it; events are
//! in `db_events.rs`, the view tree in `db_render.rs`.

use std::collections::BTreeMap;
use std::path::Path;

use arcode_plugin::prelude::*;
use arcode_plugin::ui::Tone;

use crate::db::{ColumnInfo, Database, Object};
use crate::grid::DataGrid;
use crate::query::Outcome;
use crate::recent;
use crate::sql::OpenMode;

/// Shown next to a snapshot (§10).
pub const SNAPSHOT_NOTE: &str = "snapshot; uncheckpointed WAL changes not shown";

/// Where the Query tab is.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryState {
    /// Nothing run yet.
    Idle,
    /// A query is running (rendered before it starts).
    Running,
    /// Finished: the summary line.
    Done(String),
    /// Failed or cancelled: the message, shown inline.
    Failed(String),
}

/// What opening the view should show first (from the sidebar or a command).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intent {
    /// Table or view to select.
    pub table: Option<String>,
    /// Show the Query tab.
    pub query: bool,
}

/// One database view.
#[derive(Debug)]
pub struct DbView {
    pub(crate) panel: Panel,
    /// Project-relative path of the file.
    pub path: String,
    pub(crate) db: Result<Database, String>,
    /// Tables, then views.
    pub objects: Vec<Object>,
    /// Expanded tree entries and their columns (or why they failed).
    pub expanded: BTreeMap<usize, Result<Vec<ColumnInfo>, String>>,
    /// Selected object (index into `objects`).
    pub selected: Option<usize>,
    /// The Data tab's grid.
    pub grid: Option<DataGrid>,
    /// Inline error of the Data tab.
    pub grid_error: Option<String>,
    pub(crate) next_generation: u32,
    /// Active tab (0 Data, 1 Query) as last reported or set.
    pub tab: u32,
    /// The SQL text as last delivered by the host (or set by the plugin).
    pub sql: String,
    /// The user turned writes on and they took effect.
    pub writes: bool,
    /// Feedback for the writes toggle.
    pub notice: Option<(Tone, String)>,
    /// Query tab state.
    pub query: QueryState,
    /// The last result set.
    pub results: Option<Outcome>,
    pub(crate) result_generation: u32,
    /// Recent queries, newest first.
    pub recent: Vec<String>,
}

impl DbView {
    /// Opens `path` in `panel` and renders. Open errors are shown in the
    /// panel, not returned.
    pub fn open(panel: Panel, root: &Path, path: String, intent: Intent) -> Result<Self> {
        let mut view = Self {
            panel,
            db: Database::open(root, &path, false).map_err(|e| e.message()),
            path,
            objects: Vec::new(),
            expanded: BTreeMap::new(),
            selected: None,
            grid: None,
            grid_error: None,
            next_generation: 1,
            tab: u32::from(intent.query),
            sql: String::new(),
            writes: false,
            notice: None,
            query: QueryState::Idle,
            results: None,
            result_generation: 1,
            recent: recent::load(),
        };
        view.panel.set_title(view.file_name());
        view.load_schema(intent.table.as_deref());
        view.render()?;
        Ok(view)
    }

    /// The node id of the element keyed `key` in the last rendered view
    /// (`sql`, `run`, `writes`, `data`, `result`, `schema`, `tabs`, ...).
    pub fn node(&self, key: &str) -> Option<NodeId> {
        self.panel.node_id(key)
    }

    /// The file name, for titles.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// How the file is open, if it is.
    pub fn mode(&self) -> Option<OpenMode> {
        self.db.as_ref().ok().map(Database::mode)
    }

    /// Re-reads tables and views; keeps the selection by name, or selects
    /// `prefer`, or the first object.
    pub fn load_schema(&mut self, prefer: Option<&str>) {
        let keep = prefer
            .map(str::to_owned)
            .or_else(|| self.selected_object().map(|o| o.name.clone()));
        self.expanded.clear();
        self.objects = match self.db.as_mut() {
            Ok(db) => db.objects().unwrap_or_else(|e| {
                self.grid_error = Some(e.message());
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        let index = keep
            .and_then(|name| self.objects.iter().position(|o| o.name == name))
            .or((!self.objects.is_empty()).then_some(0));
        self.select(index);
    }

    /// The selected object.
    pub fn selected_object(&self) -> Option<&Object> {
        self.selected.and_then(|i| self.objects.get(i))
    }

    /// Selects an object and loads its grid.
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|&i| i < self.objects.len());
        self.grid = None;
        self.grid_error = None;
        let (Some(object), Ok(db)) = (self.selected_object().cloned(), self.db.as_mut()) else {
            return;
        };
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        match DataGrid::load(db, object, generation) {
            Ok(grid) => self.grid = Some(grid),
            Err(e) => self.grid_error = Some(e.message()),
        }
    }

    /// Expands or collapses the columns of object `index`.
    pub fn toggle(&mut self, index: usize) {
        if self.expanded.remove(&index).is_some() {
            return;
        }
        let (Some(object), Ok(db)) = (self.objects.get(index), self.db.as_mut()) else {
            return;
        };
        let columns = db.columns(&object.name).map_err(|e| e.message());
        self.expanded.insert(index, columns);
    }

    /// Turns writes on or off by reopening the file. Writes need a
    /// non-snapshot file *and* a read-write workspace grant.
    pub fn set_writes(&mut self, root: &Path, on: bool) {
        if on && self.mode() == Some(OpenMode::Snapshot) {
            self.writes = false;
            self.notice = Some((
                Tone::Warning,
                format!("Writes are unavailable: this WAL database is open as a {SNAPSHOT_NOTE}."),
            ));
            return;
        }
        let reopened = Database::open(root, &self.path, on);
        self.writes = reopened.as_ref().is_ok_and(Database::writable);
        self.notice = match (&reopened, on, self.writes) {
            // The old connection stays in use.
            (Err(e), _, _) => Some((Tone::Danger, e.message())),
            (Ok(_), true, false) => Some((
                Tone::Warning,
                "Writes are unavailable: this project is shared read-only with the plugin. \
                 Allow writing in Plugins → Database Viewer → Permissions."
                    .to_owned(),
            )),
            (Ok(_), true, true) => Some((
                Tone::Danger,
                "Writes are on. The file is not locked: close other programs using it first."
                    .to_owned(),
            )),
            (Ok(_), false, _) => None,
        };
        match reopened {
            Ok(db) => self.db = Ok(db),
            // Writes follow the connection that stays in use.
            Err(_) => self.writes = self.db.as_ref().is_ok_and(Database::writable),
        }
    }

    /// Re-reads the schema and re-counts the grid (refresh, after writes).
    pub fn refresh(&mut self) {
        self.load_schema(None);
    }

    /// Sends the current view to the host.
    pub fn render(&mut self) -> Result<()> {
        let tree = super::db_render::view(self);
        self.panel.rerender(&tree)
    }
}
