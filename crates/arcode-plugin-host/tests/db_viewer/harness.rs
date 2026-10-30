//! A headless app around the real `arcode.db-viewer` plugin: a
//! `PluginManager` with the built plugin as a bundled plugin, a temp
//! project holding the fixture databases, scripted consent, and readers for
//! the panel snapshots the app would draw.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use arcode_plugin_host::bindings::PanelId;
use arcode_plugin_host::bindings::ui::{Event, EventKind, RowPage, TableProps, TreeItem, Widget};
use arcode_plugin_host::bridge::NoHttp;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::rows::PageRequest;
use arcode_plugin_host::ui_tree::PanelSnapshot;
use arcode_plugin_host::{
    AppServices, CallOutput, CallResult, InstanceHandle, ManagerConfig, Pending, PluginId,
    PluginManager, PluginOutput, ProjectContext,
};

/// `plugins/db-viewer/tests/fixtures`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/db-viewer/tests/fixtures")
}

/// `target/wasm-plugins` (or `$ARCODE_WASM_PLUGINS`), checked to hold the
/// built plugin.
pub fn wasm_plugins() -> PathBuf {
    let dir = std::env::var_os("ARCODE_WASM_PLUGINS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/wasm-plugins"),
        PathBuf::from,
    );
    assert!(
        dir.join("arcode.db-viewer/plugin.wasm").is_file(),
        "the DB viewer is not built: run scripts/build-plugins.sh db-viewer"
    );
    dir
}

/// One `.cwasm` cache for the whole test binary (compiling SQLite takes
/// seconds; every test after the first deserializes).
fn shared_cache() -> &'static PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("db-viewer-e2e-cache"))
}

/// The manager, its project and what the app received.
pub struct Viewer {
    pub manager: PluginManager,
    pub editor: Arc<MemoryEditor>,
    pub consent: Arc<ScriptedConsent>,
    pub outputs: Arc<Mutex<Vec<PluginOutput>>>,
    pub project: tempfile::TempDir,
    _dirs: tempfile::TempDir,
}

impl Viewer {
    /// A manager whose workspace prompts are answered "allow for this
    /// project", with the write checkbox ticked when `write`.
    pub fn new(write: bool) -> Self {
        let dirs = tempfile::tempdir().expect("tempdir");
        let mut paths = PluginPaths::with_bases(
            None,
            &dirs.path().join("config"),
            &dirs.path().join("data"),
            shared_cache(),
        );
        paths.bundled = vec![wasm_plugins()];
        let mut config = ManagerConfig::new(paths);
        config.persist = false;
        let consent = Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
            scope: GrantScope::Project,
            write,
        }));
        let editor = Arc::new(MemoryEditor::default());
        let app = AppServices {
            editor: editor.clone(),
            index: None,
            http: Arc::new(NoHttp),
            consent: Some(consent.clone()),
        };
        let (mut manager, rx) = PluginManager::new(config, app);
        let outputs = Arc::new(Mutex::new(Vec::new()));
        let sink = outputs.clone();
        std::thread::spawn(move || {
            while let Ok(out) = rx.recv_blocking() {
                sink.lock().expect("outputs").push(out);
            }
        });
        let project = tempfile::tempdir().expect("project");
        for name in ["sample.db", "wal.db", "wal.db-wal"] {
            std::fs::copy(fixtures().join(name), project.path().join(name)).expect(name);
        }
        manager.set_project(Some(ProjectContext {
            root: project.path().to_owned(),
            name: "demo".into(),
        }));
        Self {
            manager,
            editor,
            consent,
            outputs,
            project,
            _dirs: dirs,
        }
    }

    /// Opens `rel` the way the file tree does: opener lookup, then
    /// `open_with`; waits for `open-view`.
    pub fn open(&self, rel: &str) -> Opened {
        let opener = self
            .manager
            .opener_for(Path::new(rel))
            .expect("the sqlite opener matches");
        assert_eq!(opener.as_str(), "arcode.db-viewer/sqlite");
        let call = self.manager.open_with(&opener, rel).expect("open_with");
        assert_eq!(call.pending.wait(), Ok(CallOutput::Done));
        Opened {
            handle: self.manager.instance(&call.plugin).expect("instance"),
            plugin: call.plugin,
            panel: call.panel,
        }
    }
}

/// An open database view.
pub struct Opened {
    pub plugin: PluginId,
    pub panel: PanelId,
    pub handle: InstanceHandle,
}

impl Opened {
    /// The panel as the app would draw it now.
    pub fn snapshot(&self) -> Arc<PanelSnapshot> {
        self.handle.panel_view(self.panel).expect("rendered")
    }

    /// Every text, badge, spinner and empty-state string.
    pub fn texts(&self) -> Vec<String> {
        self.snapshot()
            .nodes
            .values()
            .filter_map(|n| match &n.widget {
                Widget::Text(t) => Some(t.text.clone()),
                Widget::Badge(b) => Some(b.text.clone()),
                Widget::Spinner(s) => Some(s.clone()),
                Widget::EmptyState((t, h)) => Some(format!("{t}: {h}")),
                _ => None,
            })
            .collect()
    }

    /// True when some string contains `needle`.
    pub fn shows(&self, needle: &str) -> bool {
        self.texts().iter().any(|t| t.contains(needle))
    }

    /// The node whose widget matches.
    pub fn find(&self, pred: impl Fn(&Widget) -> bool) -> u32 {
        self.snapshot()
            .nodes
            .values()
            .find(|n| pred(&n.widget))
            .map(|n| n.id)
            .unwrap_or_else(|| panic!("no such widget in {:?}", self.texts()))
    }

    /// The schema tree's node and items.
    pub fn tree(&self) -> (u32, Vec<TreeItem>) {
        let snapshot = self.snapshot();
        snapshot
            .nodes
            .values()
            .find_map(|n| match &n.widget {
                Widget::Tree(items) => Some((n.id, items.clone())),
                _ => None,
            })
            .expect("a schema tree")
    }

    /// The Data grid (sortable columns) or the result grid (not sortable).
    pub fn table(&self, data: bool) -> Option<(u32, TableProps)> {
        let snapshot = self.snapshot();
        snapshot.nodes.values().find_map(|n| match &n.widget {
            Widget::Table(t) if t.columns.first().is_some_and(|c| c.sortable) == data => {
                Some((n.id, t.clone()))
            }
            _ => None,
        })
    }

    /// Sends a widget event through the manager (gesture per kind).
    pub fn event(
        &self,
        v: &Viewer,
        node: u32,
        kind: EventKind,
        values: Vec<(u32, String)>,
    ) -> Pending {
        let event = Event { node, kind, values };
        v.manager
            .ui_event(&self.plugin, self.panel, event)
            .expect("ui_event")
    }

    /// Types `sql` into the query input and clicks Run; returns the call.
    pub fn run(&self, v: &Viewer, sql: &str) -> Pending {
        let input =
            self.find(|w| matches!(w, Widget::Input(i) if i.language.as_deref() == Some("sql")));
        let button = self.find(|w| matches!(w, Widget::Button(b) if b.label == "Run"));
        self.event(v, button, EventKind::Click, vec![(input, sql.to_owned())])
    }

    /// Runs `sql` and waits; panics unless the call itself succeeded.
    pub fn run_wait(&self, v: &Viewer, sql: &str) {
        let result: CallResult = self.run(v, sql).wait();
        assert_eq!(result, Ok(CallOutput::Done), "{sql}");
    }

    /// Activates (selects) schema entry `key`, e.g. `o3`.
    pub fn activate(&self, v: &Viewer, key: &str) {
        let (tree, _) = self.tree();
        let r = self
            .event(v, tree, EventKind::TreeActivate(key.into()), vec![])
            .wait();
        assert_eq!(r, Ok(CallOutput::Done));
    }

    /// Fetches rows of `table` like a table widget's `RowCache` would.
    pub fn fetch(&self, v: &Viewer, table: u32, start: u64, count: u32) -> Result<RowPage, String> {
        let request = PageRequest {
            start,
            count,
            epoch: 0,
        };
        let pending = v
            .manager
            .fetch_rows(&self.plugin, self.panel, table, request)
            .expect("fetch_rows");
        match pending.wait() {
            Ok(CallOutput::Rows(page)) => Ok(page),
            other => Err(format!("{other:?}")),
        }
    }
}
