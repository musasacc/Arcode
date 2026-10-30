//! Plan step 10: the bundled `arcode.http-client` plugin end to end under a
//! headless `PluginManager`, with a scripted `ConsentProvider`, an
//! in-memory editor and the real ureq transport against a loopback server
//! (never the external network).
//!
//! Needs the release build: run `scripts/build-plugins.sh http-client`
//! (output in `target/wasm-plugins`, or `$ARCODE_WASM_PLUGINS`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::ui::Widget;
use arcode_plugin_host::bridge::DocSnapshot;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::net::{TransportOptions, UreqTransport};
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::{
    AppServices, CallOutput, InstanceHandle, ManagerConfig, PluginId, PluginManager, PluginOutput,
    ProjectContext, QualifiedId,
};
use serde_json::json;

use crate::loopback::{Reply, Server};

const DOC: u64 = 1;

const ALLOW_PROJECT: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Project,
    write: false,
};

/// The `.http` file under test; `{{base}}` comes from the environment.
const FILE: &str = "\
@item = widget

### Create an item
# @name create
POST {{base}}/items?x=1
Content-Type: application/json
Authorization: Bearer {{token}}

{\"name\": \"{{item}}\", \"id\": \"{{$uuid}}\"}

### Text
GET {{base}}/page

### Sniffed
GET {{base}}/sniff

### Slow
GET {{base}}/slow
";

/// `target/wasm-plugins` (or `$ARCODE_WASM_PLUGINS`).
fn bundled_root() -> PathBuf {
    let root = std::env::var_os("ARCODE_WASM_PLUGINS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/wasm-plugins"),
        PathBuf::from,
    );
    assert!(
        root.join("arcode.http-client/plugin.wasm").is_file(),
        "the HTTP client plugin is not built: run scripts/build-plugins.sh http-client"
    );
    root
}

struct Client {
    manager: PluginManager,
    consent: Arc<ScriptedConsent>,
    outputs: Arc<Mutex<Vec<PluginOutput>>>,
    server: Server,
    _project: tempfile::TempDir,
    _dirs: tempfile::TempDir,
}

/// A manager with only bundled plugins, a project holding `FILE` and its
/// environment files, and the document open in the editor.
fn client(consent: ScriptedConsent, server: Server) -> Client {
    let dirs = tempfile::tempdir().unwrap();
    let base = dirs.path();
    let mut paths = PluginPaths::with_bases(
        None,
        &base.join("config"),
        &base.join("data"),
        &base.join("cache"),
    );
    paths.bundled = vec![bundled_root()];
    let project = tempfile::tempdir().unwrap();
    let env = json!({"local": {"base": server.url("")}, "other": {"base": "http://nowhere"}});
    std::fs::write(project.path().join("http-client.env.json"), env.to_string()).unwrap();
    let private = json!({"local": {"token": "s3cret"}});
    std::fs::write(
        project.path().join("http-client.private.env.json"),
        private.to_string(),
    )
    .unwrap();
    std::fs::write(project.path().join("api.http"), FILE).unwrap();
    let editor = Arc::new(MemoryEditor::default());
    editor.publish(DocSnapshot {
        id: DOC,
        path: Some("api.http".into()),
        language: "http".into(),
        version: 1,
        rope: ropey::Rope::from_str(FILE),
        selections: vec![],
    });
    let consent = Arc::new(consent);
    let transport = UreqTransport::new(&TransportOptions {
        use_env_proxy: false,
        ..TransportOptions::default()
    });
    let app = AppServices {
        editor,
        index: None,
        http: Arc::new(transport),
        consent: Some(consent.clone()),
    };
    let (mut manager, rx) = PluginManager::new(ManagerConfig::new(paths), app);
    manager.set_project(Some(ProjectContext {
        root: project.path().to_owned(),
        name: "demo".into(),
    }));
    // The headless app: picks the second quick-pick item ("local").
    let outputs: Arc<Mutex<Vec<PluginOutput>>> = Arc::default();
    let sink = outputs.clone();
    std::thread::spawn(move || {
        while let Ok(out) = rx.recv_blocking() {
            match out {
                PluginOutput::QuickPick { reply, .. } => reply.send(Some(1)),
                other => sink.lock().unwrap().push(other),
            }
        }
    });
    Client {
        manager,
        consent,
        outputs,
        server,
        _project: project,
        _dirs: dirs,
    }
}

fn id() -> PluginId {
    PluginId::parse("arcode.http-client").unwrap()
}

fn cmd(name: &str) -> QualifiedId {
    QualifiedId::parse(&format!("arcode.http-client/{name}")).unwrap()
}

impl Client {
    fn instance(&self) -> InstanceHandle {
        self.manager.instance(&id()).unwrap()
    }

    /// The lens args per request, in file order.
    fn lenses(&self) -> Vec<(u32, String, String)> {
        let calls = self.manager.code_lenses(DOC);
        assert_eq!(calls.len(), 1, "one lens provider");
        match calls[0].pending.wait() {
            Ok(CallOutput::Lenses(l)) => l.into_iter().map(|l| (l.line, l.title, l.args)).collect(),
            other => panic!("lenses: {other:?}"),
        }
    }

    fn select_environment(&self) {
        let p = self
            .manager
            .run_command(&cmd("select-environment"), "null", Some(DOC));
        assert_eq!(p.unwrap().wait(), Ok(CallOutput::Done));
    }

    /// Runs `send` for the request on `line`, as a lens click.
    fn send(&self, line: u32) -> arcode_plugin_host::Pending {
        let args = json!({"doc": DOC, "line": line}).to_string();
        self.manager
            .run_command(&cmd("send"), &args, Some(DOC))
            .unwrap()
    }

    /// Every widget of the Response panel.
    fn panel(&self) -> Vec<Widget> {
        let h = self.instance();
        let panels = h.open_panels();
        assert_eq!(panels.len(), 1, "{panels:?}");
        let snapshot = h.panel_view(panels[0].0).expect("panel view");
        snapshot.nodes.values().map(|n| n.widget.clone()).collect()
    }

    /// Waits (up to 5 s) until the app received an output matching `pred`;
    /// outputs travel through the drain thread, so they may lag a reply.
    fn saw(&self, pred: impl Fn(&PluginOutput) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.outputs.lock().unwrap().iter().any(&pred) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn routes(s: &crate::loopback::Seen) -> Reply {
    match s.path.as_str() {
        "/page" => Reply {
            status: 404,
            headers: vec![("content-type".into(), "text/html".into())],
            body: b"<h1>missing</h1>".to_vec(),
            delay: Duration::ZERO,
        },
        "/sniff" => Reply::text(200, r#"[{"sniffed": true}]"#),
        "/slow" => Reply {
            delay: Duration::from_secs(10),
            ..Reply::text(200, "late")
        },
        _ => Reply {
            status: 201,
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-reply".into(), "yes".into()),
            ],
            body: br#"{"created": true}"#.to_vec(),
            delay: Duration::from_millis(20),
        },
    }
}

mod tests;
