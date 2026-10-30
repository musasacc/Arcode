//! `LspManager` tests. This binary has its own `main` (harness = false):
//! with `ARCODE_FAKE_LSP` set it acts as a fake language server, which the
//! tests launch by pointing a `ServerConfig` at `current_exe()`.

mod fake_lsp;

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use arcode_lsp::lsp_types::request::{Completion, HoverRequest};
use arcode_lsp::lsp_types::{
    CompletionParams, CompletionResponse, HoverContents, HoverParams, MarkupContent, Position,
    Range, TextDocumentIdentifier, TextDocumentPositionParams,
};
use arcode_lsp::{
    EventReceiver, LspError, LspEvent, LspManager, PositionEncoding, ServerConfig, ServerStatus,
    TextChange, path_to_uri,
};

/// Minimal executor that is definitely not tokio.
fn block_on<F: Future>(fut: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::park_timeout(Duration::from_millis(100));
    }
}

/// Buffers events so waiting for one does not discard the others.
struct Events {
    rx: EventReceiver,
    seen: Vec<LspEvent>,
}

impl Events {
    fn new(rx: EventReceiver) -> Self {
        Events {
            rx,
            seen: Vec::new(),
        }
    }

    /// Removes and returns the first event matching `pred`, polling the
    /// channel (without any async runtime) for up to 20 seconds.
    fn wait_for(&mut self, what: &str, pred: impl Fn(&LspEvent) -> bool) -> LspEvent {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            while let Ok(ev) = self.rx.try_recv() {
                self.seen.push(ev);
            }
            if let Some(i) = self.seen.iter().position(&pred) {
                return self.seen.remove(i);
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; seen: {:#?}",
                self.seen
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for a diagnostics event whose first message is `message`.
    fn wait_diag(&mut self, message: &str) -> PathBuf {
        let ev = self.wait_for(message, |e| {
            diag_message(e).is_some_and(|(_, _, m)| m == message)
        });
        diag_message(&ev)
            .map(|(_, p, _)| p.to_path_buf())
            .unwrap_or_default()
    }
}

fn fake_config(name: &str, encoding: &str) -> ServerConfig {
    let exe = std::env::current_exe().expect("current exe");
    ServerConfig {
        name: name.into(),
        command: exe.to_string_lossy().into_owned(),
        args: vec![],
        env: vec![
            ("ARCODE_FAKE_LSP".into(), "1".into()),
            ("ARCODE_FAKE_NAME".into(), name.into()),
            ("ARCODE_FAKE_ENCODING".into(), encoding.into()),
        ],
        root_markers: vec!["project.marker".into()],
        initialization_options: None,
    }
}

fn diag_message(ev: &LspEvent) -> Option<(&str, &Path, &str)> {
    match ev {
        LspEvent::Diagnostics {
            server,
            path,
            diagnostics,
            ..
        } => diagnostics
            .first()
            .map(|d| (server.as_str(), path.as_path(), d.message.as_str())),
        _ => None,
    }
}

fn hover_params(path: &Path) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(path_to_uri(path)),
            position: Position::new(0, 0),
        },
        work_done_progress_params: Default::default(),
    }
}

fn hover_text(contents: &HoverContents) -> &str {
    match contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        _ => "",
    }
}

fn missing_binary_reports_failure() {
    let mut config = ServerConfig::new("ghost", "arcode-definitely-missing-server-xyz", &[".git"]);
    config.args = vec!["--stdio".into()];
    let languages = HashMap::from([("ghostlang".to_string(), vec!["ghost".to_string()])]);
    let (manager, rx) = LspManager::new(vec![config], languages);
    let mut rx = Events::new(rx);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.ghost");
    manager.open_document(&file, "ghostlang", 1, "text".to_string());
    let ev = rx.wait_for("Failed status", |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Failed(_),
                ..
            }
        )
    });
    let LspEvent::ServerStatus {
        server,
        status: ServerStatus::Failed(msg),
        root,
    } = ev
    else {
        unreachable!()
    };
    assert_eq!(server, "ghost");
    assert_eq!(root, dir.path());
    assert!(
        msg.contains("arcode-definitely-missing-server-xyz not found on PATH"),
        "{msg}"
    );
    // Everything else keeps working and resolves instead of hanging.
    manager.change_document(&file, 2, vec![], "x".to_string());
    manager.save_document(&file);
    let r = block_on(manager.request::<HoverRequest>(&file, hover_params(&file), |_| true));
    assert_eq!(r.unwrap_err(), LspError::NoServer);
    assert!(
        block_on(manager.request_all::<HoverRequest>(&file, hover_params(&file), |_| true))
            .is_empty()
    );
    manager.close_document(&file);
    // Unknown languages are ignored.
    manager.open_document(&file, "nolang", 1, "x".to_string());
    manager.shutdown_all();
}

fn two_servers_priority_fanout_and_encodings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("project.marker"), "").unwrap();
    let file = root.join("src/main file.fk");
    let languages = HashMap::from([(
        "fake".to_string(),
        vec!["alpha".to_string(), "beta".to_string()],
    )]);
    let (manager, rx) = LspManager::new(
        vec![fake_config("alpha", "utf-16"), fake_config("beta", "utf-8")],
        languages,
    );
    let mut rx = Events::new(rx);
    manager.open_document(&file, "fake", 1, "hello".to_string());
    assert_eq!(rx.wait_diag("alpha: opened v1"), file);
    rx.wait_diag("beta: opened v1");
    rx.wait_for(
        "stderr log",
        |e| matches!(e, LspEvent::Log { message, .. } if message == "alpha starting"),
    );
    assert_eq!(
        manager.server_instances(),
        vec![
            ("alpha".into(), root.clone()),
            ("beta".into(), root.clone())
        ]
    );
    assert_eq!(manager.running_servers(&file), vec!["alpha", "beta"]);
    assert_eq!(
        manager.position_encoding(&file, "beta"),
        Some(PositionEncoding::Utf8)
    );
    assert!(
        manager
            .capabilities(&file, "alpha")
            .unwrap()
            .hover_provider
            .is_some()
    );

    // UTF-16 ranges: alpha (utf-16) gets them, beta (utf-8) gets full text.
    let change = TextChange {
        range: Some(Range::new(Position::new(0, 0), Position::new(0, 5))),
        text: "bye".into(),
    };
    manager.change_document(&file, 2, vec![change], "bye".to_string());
    rx.wait_diag("alpha: changed v2 incremental");
    rx.wait_diag("beta: changed v2 full");

    // The request is queued behind the change, so the server saw v2.
    let (server, hover) = block_on(manager.request::<HoverRequest>(
        &file,
        hover_params(&file),
        |c| c.hover_provider.is_some(),
    ))
    .unwrap();
    assert_eq!(server, "alpha");
    assert_eq!(hover_text(&hover.unwrap().contents), "alpha v2 incremental");

    // Capability filter skips alpha when asked to.
    let (server, _) = block_on(
        manager.request::<HoverRequest>(&file, hover_params(&file), |c| {
            c.position_encoding
                .as_ref()
                .is_some_and(|e| e.as_str() == "utf-8")
        }),
    )
    .unwrap();
    assert_eq!(server, "beta");

    // Fan-out to every server.
    let params = CompletionParams {
        text_document_position: hover_params(&file).text_document_position_params,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let all = block_on(
        manager.request_all::<Completion>(&file, params, |c| c.completion_provider.is_some()),
    );
    let labels: Vec<(String, String)> = all
        .into_iter()
        .map(|(s, r)| match r {
            Some(CompletionResponse::Array(items)) => (s, items[0].label.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            ("alpha".into(), "alpha".into()),
            ("beta".into(), "beta".into())
        ]
    );

    // Save includes the latest text; close clears diagnostics.
    manager.save_document(&file);
    rx.wait_diag("alpha: saved bye");
    manager.close_document(&file);
    rx.wait_for(
        "cleared diagnostics",
        |e| matches!(e, LspEvent::Diagnostics { diagnostics, .. } if diagnostics.is_empty()),
    );

    manager.shutdown_all();
    for name in ["alpha", "beta"] {
        rx.wait_for("Stopped", |e| {
            matches!(e, LspEvent::ServerStatus { server, status: ServerStatus::Stopped, .. } if server == name)
        });
    }
    assert!(manager.server_instances().is_empty());
}

fn one_instance_per_root() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    for r in [&a, &b] {
        std::fs::create_dir_all(r).unwrap();
        std::fs::write(r.join("project.marker"), "").unwrap();
    }
    let languages = HashMap::from([("fake".to_string(), vec!["alpha".to_string()])]);
    let (manager, rx) = LspManager::new(vec![fake_config("alpha", "utf-16")], languages);
    let mut rx = Events::new(rx);
    manager.open_document(&a.join("1.fk"), "fake", 1, "x".to_string());
    manager.open_document(&a.join("2.fk"), "fake", 1, "x".to_string());
    manager.open_document(&b.join("3.fk"), "fake", 1, "x".to_string());
    for _ in 0..3 {
        rx.wait_diag("alpha: opened v1");
    }
    assert_eq!(
        manager.server_instances(),
        vec![("alpha".into(), a.clone()), ("alpha".into(), b.clone())]
    );
    // Files outside any marker fall back to the workspace root.
    let loose = dir.path().join("loose/4.fk");
    manager.set_workspace_root(Some(dir.path().to_path_buf()));
    manager.open_document(&loose, "fake", 1, "x".to_string());
    rx.wait_diag("alpha: opened v1");
    assert_eq!(manager.server_instances().len(), 3);
    assert!(
        manager
            .server_instances()
            .contains(&("alpha".into(), dir.path().to_path_buf()))
    );
    // Dropping the manager without shutdown must not hang or panic.
    drop(manager);
}

fn lazy_full_text_only_built_when_needed() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.marker"), "").unwrap();
    let (solo, mixed) = (dir.path().join("solo.fk"), dir.path().join("mixed.fk"));
    let languages = HashMap::from([
        ("solo".to_string(), vec!["alpha".to_string()]),
        (
            "mixed".to_string(),
            vec!["alpha".to_string(), "beta".to_string()],
        ),
    ]);
    let (manager, rx) = LspManager::new(
        vec![fake_config("alpha", "utf-16"), fake_config("beta", "utf-8")],
        languages,
    );
    let mut rx = Events::new(rx);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = |text: &'static str| {
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            text.to_string()
        }
    };
    let change = || {
        vec![TextChange {
            range: Some(Range::new(Position::new(0, 0), Position::new(0, 1))),
            text: "y".into(),
        }]
    };

    // Only an incremental UTF-16 server: the full text is never built,
    // not even while the change waits for initialization.
    manager.open_document(&solo, "solo", 1, "x".to_string());
    manager.change_document_lazy(&solo, 2, change(), PositionEncoding::Utf16, counted("y"));
    rx.wait_diag("alpha: changed v2 incremental");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // didSave with includeText needs it: built once, then.
    manager.save_document(&solo);
    rx.wait_diag("alpha: saved y");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // A UTF-8 server needs full text for UTF-16 ranges: built exactly once
    // and shared with every server that needs it.
    calls.store(0, Ordering::SeqCst);
    manager.open_document(&mixed, "mixed", 1, "x".to_string());
    manager.change_document_lazy(&mixed, 2, change(), PositionEncoding::Utf16, counted("y"));
    rx.wait_diag("alpha: changed v2 incremental");
    rx.wait_diag("beta: changed v2 full");
    manager.save_document(&mixed);
    rx.wait_diag("alpha: saved y");
    rx.wait_diag("beta: saved y");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    manager.shutdown_all();
}

/// `open_document_lazy` builds the text once per server for `didOpen` and
/// again only for a `didSave` that includes it; nothing else keeps a copy.
fn lazy_open_builds_text_on_demand() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.marker"), "").unwrap();
    let file = dir.path().join("lazy.fk");
    let languages = HashMap::from([(
        "mixed".to_string(),
        vec!["alpha".to_string(), "beta".to_string()],
    )]);
    let (manager, rx) = LspManager::new(
        vec![fake_config("alpha", "utf-16"), fake_config("beta", "utf-8")],
        languages,
    );
    let mut rx = Events::new(rx);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    manager.open_document_lazy(&file, "mixed", 1, move || {
        counter.fetch_add(1, Ordering::SeqCst);
        "big".to_string()
    });
    rx.wait_diag("alpha: opened v1");
    rx.wait_diag("beta: opened v1");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "once per server");
    manager.save_document(&file);
    rx.wait_diag("alpha: saved big");
    rx.wait_diag("beta: saved big");
    assert_eq!(calls.load(Ordering::SeqCst), 3, "once more for didSave");
    manager.shutdown_all();
}

fn main() {
    if std::env::var_os("ARCODE_FAKE_LSP").is_some() {
        fake_lsp::run();
        return;
    }
    let tests: [(&str, fn()); 5] = [
        (
            "missing_binary_reports_failure",
            missing_binary_reports_failure,
        ),
        (
            "two_servers_priority_fanout_and_encodings",
            two_servers_priority_fanout_and_encodings,
        ),
        ("one_instance_per_root", one_instance_per_root),
        (
            "lazy_full_text_only_built_when_needed",
            lazy_full_text_only_built_when_needed,
        ),
        (
            "lazy_open_builds_text_on_demand",
            lazy_open_builds_text_on_demand,
        ),
    ];
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let mut ran = 0;
    for (name, test) in tests {
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        println!("test {name} ...");
        test();
        println!("test {name} ... ok");
        ran += 1;
    }
    println!("manager tests: {ran} passed");
}
