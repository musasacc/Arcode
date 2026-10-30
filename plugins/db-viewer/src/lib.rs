//! `arcode.db-viewer`: browse and query SQLite databases in the project
//! (docs/PLUGINS.md §10).
//!
//! SQLite runs inside the plugin (rusqlite `bundled`, built for
//! `wasm32-wasip2` by `scripts/build-plugins.sh`) and reads files through
//! the WASI preopen at `/project`. The same code builds natively, where the
//! SDK's `testing` fakes answer every host import and `DbViewer::root`
//! points at a fixture directory.
//!
//! - `open-view("sqlite", {path})` (opener) shows a [`view::DbView`].
//! - `open-view("databases")` (sidebar) shows a [`view::Sidebar`].
//! - Commands: `refresh` (re-read every open view) and `new-query`.

pub mod cells;
pub mod db;
pub mod grid;
pub mod paging;
pub mod query;
pub mod recent;
pub mod sql;
pub mod view;

use std::collections::BTreeMap;
use std::path::PathBuf;

use arcode_plugin::prelude::*;
use arcode_plugin::ui::Panel;
use serde::Deserialize;

use view::{DbView, Intent, OpenRequest, Sidebar, View};

/// The plugin: its open panels and what to show in views being opened.
#[derive(Debug)]
pub struct DbViewer {
    /// Where the project is (`/project` in the sandbox; a fixture natively).
    pub root: PathBuf,
    /// Open panels.
    pub views: BTreeMap<PanelId, View>,
    /// Files asked to open (sidebar, `new-query`) and what to show then.
    pending: BTreeMap<String, Intent>,
    /// The database view used last (target of `new-query`).
    last_db: Option<PanelId>,
}

impl Default for DbViewer {
    fn default() -> Self {
        Self {
            root: PathBuf::from(workspace::PROJECT_DIR),
            views: BTreeMap::new(),
            pending: BTreeMap::new(),
            last_db: None,
        }
    }
}

#[derive(Deserialize)]
struct OpenArgs {
    path: String,
}

impl DbViewer {
    /// A plugin whose project lives at `root` (native tests).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            ..Self::default()
        }
    }

    /// The database view in `panel`, if that is one.
    pub fn db_view(&mut self, panel: PanelId) -> Option<&mut DbView> {
        match self.views.get_mut(&panel) {
            Some(View::Db(v)) => Some(v),
            _ => None,
        }
    }

    /// Asks the editor to open `request.path` (the `sqlite` opener then
    /// shows it) and remembers what that view should show.
    fn open_file(&mut self, request: OpenRequest) -> Result<()> {
        editor::open_file(&request.path, None)
            .map_err(|e| Error::msg(format!("could not open {}: {e:?}", request.path)))?;
        self.pending.insert(request.path, request.intent);
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        for view in self.views.values_mut() {
            match view {
                View::Db(v) => {
                    v.refresh();
                    v.render()?;
                }
                View::Sidebar(s) => s.refresh()?,
            }
        }
        Ok(())
    }

    fn new_query(&mut self) -> Result<()> {
        if let Some(view) = self.last_db.and_then(|p| self.db_view(p)) {
            return view.show_query();
        }
        let files = view::sidebar::find_databases().map_err(Error::msg)?;
        let index = match files.len() {
            0 => {
                host::notify(host::Level::Info, "No SQLite databases in this project.");
                return Ok(());
            }
            1 => 0,
            _ => match host::quick_pick("Open a database", &files) {
                Some(i) => i as usize,
                None => return Ok(()),
            },
        };
        let Some(path) = files.get(index) else {
            return Ok(());
        };
        self.open_file(OpenRequest {
            path: path.clone(),
            intent: Intent {
                table: None,
                query: true,
            },
        })
    }
}

impl Plugin for DbViewer {
    fn command(&mut self, id: &str, _args: Json) -> Result<()> {
        match id {
            "refresh" => self.refresh(),
            "new-query" => self.new_query(),
            _ => Err(Error::msg(format!("unknown command `{id}`"))),
        }
    }

    fn open_view(&mut self, panel: PanelId, contribution: &str, args: Json) -> Result<()> {
        let view = match contribution {
            "sqlite" => {
                let path = args.parse::<OpenArgs>()?.path;
                let intent = self.pending.remove(&path).unwrap_or_default();
                self.last_db = Some(panel);
                View::Db(Box::new(DbView::open(
                    Panel::attach(panel),
                    &self.root,
                    path,
                    intent,
                )?))
            }
            "databases" => View::Sidebar(Sidebar::open(Panel::attach(panel))?),
            other => return Err(Error::msg(format!("no view for `{other}`"))),
        };
        self.views.insert(panel, view);
        Ok(())
    }

    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        let root = self.root.clone();
        let request = match self.views.get_mut(&panel) {
            Some(View::Db(v)) => {
                self.last_db = Some(panel);
                v.handle(&root, event)?;
                None
            }
            Some(View::Sidebar(s)) => s.handle(&root, event)?,
            None => return Err(Error::msg(format!("no panel {panel}"))),
        };
        match request {
            Some(request) => self.open_file(request),
            None => Ok(()),
        }
    }

    fn fetch_rows(
        &mut self,
        panel: PanelId,
        table: NodeId,
        start: u64,
        count: u32,
    ) -> Result<RowPage> {
        match self.db_view(panel) {
            Some(v) => v.fetch(table, start, count),
            None => Err(Error::msg(format!("no table in panel {panel}"))),
        }
    }

    fn view_closed(&mut self, panel: PanelId) {
        self.views.remove(&panel);
        if self.last_db == Some(panel) {
            self.last_db = None;
        }
    }
}

#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(DbViewer);
