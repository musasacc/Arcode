//! Shared harness for the fixture tests: fixture lookup, a headless "app"
//! that drains plugin output, and fakes for every bridge trait.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::editor::{EditorError, TextEdit};
use arcode_plugin_host::bindings::http::{HttpError, Request, Response, Timing};
use arcode_plugin_host::bindings::{DocId, Position, Range};
use arcode_plugin_host::bridge::{DocSnapshot, EditorBridge, HttpTransport, ProjectIndex};
use arcode_plugin_host::cancel::CancelToken;
use arcode_plugin_host::consent::{ConsentProvider, Permissions};
use arcode_plugin_host::grant_store::GrantStore;
use arcode_plugin_host::manifest::UserLimits;
use arcode_plugin_host::registry::load;
use arcode_plugin_host::{
    Call, CallResult, InstanceHandle, InstanceSpec, OutputSender, PluginOutput, PluginSource,
    ProjectContext, RegisteredPlugin, Runtime, RuntimeConfig, Services,
};
use serde_json::Value;

/// Serialises tests whose timing would suffer from parallel CPU load.
pub fn timing_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `target/test-plugins` (or `$ARCODE_TEST_PLUGINS`).
fn fixtures_dir() -> PathBuf {
    std::env::var_os("ARCODE_TEST_PLUGINS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-plugins"),
        PathBuf::from,
    )
}

/// A built fixture as a registered dev plugin.
pub fn fixture(name: &str) -> RegisteredPlugin {
    let dir = fixtures_dir().join(name);
    assert!(
        dir.join("plugin.wasm").is_file(),
        "fixture `{name}` is not built: run scripts/build-test-plugins.sh"
    );
    let manifest = load(&dir, PluginSource::Dev).expect("fixture manifest");
    let limits = manifest.limits.clamp(&UserLimits::default());
    RegisteredPlugin {
        manifest,
        dir,
        source: PluginSource::Dev,
        limits,
        shadowed: Vec::new(),
    }
}

/// A runtime with a private cache dir and test-sized timings.
pub fn runtime(tweak: impl FnOnce(&mut RuntimeConfig)) -> (Arc<Runtime>, tempfile::TempDir) {
    let cache = tempfile::tempdir().expect("tempdir");
    let mut config = RuntimeConfig::new(cache.path().join("cwasm"));
    tweak(&mut config);
    (Arc::new(Runtime::new(config)), cache)
}

/// The headless app: answers quick picks and records every other output.
pub struct App {
    pub outputs: Arc<Mutex<Vec<PluginOutput>>>,
    pub picks: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    pub sender: OutputSender,
}

impl App {
    /// Starts draining; quick picks are answered with `pick`.
    pub fn start(pick: Option<u32>) -> Self {
        let (sender, rx) = OutputSender::channel();
        let outputs = Arc::new(Mutex::new(Vec::new()));
        let picks = Arc::new(Mutex::new(Vec::new()));
        let (o, p) = (outputs.clone(), picks.clone());
        std::thread::spawn(move || {
            while let Ok(out) = rx.recv_blocking() {
                match out {
                    PluginOutput::QuickPick {
                        title,
                        items,
                        reply,
                        ..
                    } => {
                        p.lock().unwrap().push((title, items));
                        reply.send(pick);
                    }
                    other => o.lock().unwrap().push(other),
                }
            }
        });
        Self {
            outputs,
            picks,
            sender,
        }
    }

    /// Waits until an output matching `pred` arrived.
    pub fn wait_for(&self, pred: impl Fn(&PluginOutput) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.outputs.lock().unwrap().iter().any(&pred) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
}

/// Services with the given fakes; no project grants on disk.
pub fn services(
    app: &App,
    consent: Arc<dyn ConsentProvider>,
    editor: Arc<dyn EditorBridge>,
    http: Arc<dyn HttpTransport>,
) -> Services {
    Services {
        editor,
        index: Arc::new(FakeIndex(vec!["hello.txt".into(), "src/lib.rs".into()])),
        http,
        permissions: Permissions::new(GrantStore::in_memory(true), consent),
        output: app.sender.clone(),
    }
}

/// An instance handle for `plugin`.
pub fn handle(
    runtime: &Arc<Runtime>,
    plugin: RegisteredPlugin,
    services: Services,
    project: Option<&Path>,
) -> InstanceHandle {
    let mut user_settings = serde_json::Map::new();
    user_settings.insert("greeting".into(), Value::from("hi"));
    user_settings.insert("not-declared".into(), Value::from(1));
    let spec = InstanceSpec {
        plugin,
        project: project.map(|root| ProjectContext {
            root: root.to_owned(),
            name: "proj".into(),
        }),
        user_settings,
        services,
    };
    InstanceHandle::new(runtime.clone(), spec)
}

/// Runs the manifest command `run`/`echo` with JSON args and waits.
pub fn command(h: &InstanceHandle, id: &str, args: Value, gesture: bool) -> CallResult {
    h.dispatch(
        Call::Command {
            command: id.into(),
            args: args.to_string(),
        },
        gesture,
    )
    .wait()
}

/// Every `host.log` line that is a JSON object.
pub fn json_lines(h: &InstanceHandle) -> Vec<Value> {
    h.log()
        .lines()
        .into_iter()
        .filter_map(|l| serde_json::from_str::<Value>(&l.text).ok())
        .filter(Value::is_object)
        .collect()
}

/// The last JSON line whose `key` equals `value`.
pub fn last_with(h: &InstanceHandle, key: &str, value: &str) -> Value {
    json_lines(h)
        .into_iter()
        .rev()
        .find(|v| v[key] == value)
        .unwrap_or_else(|| panic!("no log line with {key}={value}: {:?}", h.log().lines()))
}

/// Ranks and their texts, as in arcode-buffer.
pub fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range {
        start: Position { line: sl, column: sc },
        end: Position { line: el, column: ec },
    }
}

/// An editor with fixed snapshots that records writes.
#[derive(Default)]
pub struct FakeEditor {
    pub docs: Vec<DocSnapshot>,
    pub focused: Option<DocId>,
    pub edits: Mutex<Vec<(DocId, u64, Vec<TextEdit>)>>,
    pub opened: Mutex<Vec<(String, Option<Position>)>>,
    pub scratches: Mutex<Vec<(String, String, String)>>,
}

impl EditorBridge for FakeEditor {
    fn snapshot(&self, doc: DocId) -> Option<DocSnapshot> {
        self.docs.iter().find(|d| d.id == doc).cloned()
    }
    fn focused(&self) -> Option<DocId> {
        self.focused
    }
    fn apply_edits(&self, doc: DocId, expected: u64, edits: Vec<TextEdit>) -> Result<u64, EditorError> {
        let current = self.snapshot(doc).ok_or(EditorError::NoSuchDocument)?.version;
        if current != expected {
            return Err(EditorError::VersionMismatch(current));
        }
        self.edits.lock().unwrap().push((doc, expected, edits));
        Ok(current + 1)
    }
    fn open_file(&self, path: &str, at: Option<Position>) -> Result<(), EditorError> {
        self.opened.lock().unwrap().push((path.into(), at));
        Ok(())
    }
    fn open_scratch(&self, title: &str, language: &str, text: &str) -> Result<DocId, EditorError> {
        self.scratches
            .lock()
            .unwrap()
            .push((title.into(), language.into(), text.into()));
        Ok(99)
    }
}

/// An index with a fixed file list, filtered by the matcher.
pub struct FakeIndex(pub Vec<String>);

impl ProjectIndex for FakeIndex {
    fn find_files(&self, matcher: &globset::GlobMatcher, limit: usize) -> Vec<String> {
        self.0
            .iter()
            .filter(|p| matcher.is_match(p))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// A transport that answers every request with a canned response and asks
/// about two redirect targets, recording the host's verdicts.
#[derive(Default)]
pub struct FakeHttp {
    pub requests: Mutex<Vec<Request>>,
    pub redirects: Mutex<Vec<(String, bool)>>,
}

impl HttpTransport for FakeHttp {
    fn send(
        &self,
        request: Request,
        _cancel: &CancelToken,
        allow_redirect: &mut dyn FnMut(&str, u16) -> bool,
    ) -> Result<Response, HttpError> {
        for (host, port) in [("127.0.0.1", 9090), ("evil.test", 80)] {
            let ok = allow_redirect(host, port);
            self.redirects.lock().unwrap().push((host.into(), ok));
        }
        let body = request.body.clone();
        let url = request.url.clone();
        self.requests.lock().unwrap().push(request);
        Ok(Response {
            status: 201,
            reason: "Created".into(),
            version: "HTTP/1.1".into(),
            final_url: url,
            remote_addr: Some("127.0.0.1:8080".into()),
            headers: vec![("content-type".into(), "text/plain".into())],
            body,
            truncated: true,
            timing: Timing {
                headers_ms: 3,
                total_ms: 7,
            },
        })
    }
}
