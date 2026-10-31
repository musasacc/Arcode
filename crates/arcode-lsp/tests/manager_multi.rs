//! Several servers for one document: request fallback, per-encoding
//! changes, feature switches, live reconfiguration and restarts. This
//! binary has its own `main` (harness = false): with `ARCODE_FAKE_LSP` set
//! it acts as a fake language server (see `fake_lsp`).

mod fake_lsp;

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use arcode_lsp::lsp_types::request::HoverRequest;
use arcode_lsp::lsp_types::{
    HoverContents, HoverParams, MarkupContent, Position, Range, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use arcode_lsp::{
    EventReceiver, LspEvent, LspManager, ServerConfig, ServerStatus, TextChange, path_to_uri,
};

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

struct Events {
    rx: EventReceiver,
    seen: Vec<LspEvent>,
}

impl Events {
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

    fn wait_diag(&mut self, message: &str) {
        self.wait_for(message, |e| diag(e).as_deref() == Some(message));
    }

    /// Diagnostic messages received so far (and not yet consumed).
    fn drain_diags(&mut self) -> Vec<String> {
        while let Ok(ev) = self.rx.try_recv() {
            self.seen.push(ev);
        }
        self.seen.iter().filter_map(diag).collect()
    }
}

fn diag(ev: &LspEvent) -> Option<String> {
    match ev {
        LspEvent::Diagnostics {
            server,
            diagnostics,
            ..
        } => Some(
            diagnostics
                .first()
                .map_or_else(|| format!("{server}: cleared"), |d| d.message.clone()),
        ),
        _ => None,
    }
}

fn config(name: &str, encoding: &str, hover: &str) -> ServerConfig {
    let exe = std::env::current_exe().expect("current exe");
    ServerConfig {
        name: name.into(),
        command: exe.to_string_lossy().into_owned(),
        env: vec![
            ("ARCODE_FAKE_LSP".into(), "1".into()),
            ("ARCODE_FAKE_NAME".into(), name.into()),
            ("ARCODE_FAKE_ENCODING".into(), encoding.into()),
            ("ARCODE_FAKE_HOVER".into(), hover.into()),
        ],
        root_markers: vec!["project.marker".into()],
        ..Default::default()
    }
}

fn languages(names: &[&str]) -> HashMap<String, Vec<String>> {
    HashMap::from([(
        "fake".to_string(),
        names.iter().map(|n| n.to_string()).collect(),
    )])
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("project.marker"), "").unwrap();
    let file = root.join("main.fk");
    (dir, file)
}

fn hover(manager: &LspManager, file: &Path) -> (String, Option<String>) {
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(path_to_uri(file)),
            position: Position::new(0, 0),
        },
        work_done_progress_params: Default::default(),
    };
    let (server, hover) =
        block_on(manager.request::<HoverRequest>(file, params, |c| c.hover_provider.is_some()))
            .expect("hover answered");
    let text = hover.map(|h| match h.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        _ => String::new(),
    });
    (server, text)
}

/// A server answering `null` or failing hands the request to the next one;
/// a server with hover switched off is skipped.
fn single_answers_fall_back_down_the_list() {
    let (_dir, file) = project();
    let servers = vec![
        config("alpha", "utf-16", "null"),
        config("beta", "utf-16", "error"),
        config("gamma", "utf-16", "normal"),
    ];
    let (manager, rx) = LspManager::new(servers, languages(&["alpha", "beta", "gamma"]));
    let mut rx = Events {
        rx,
        seen: Vec::new(),
    };
    manager.open_document(&file, "fake", 1, "x".to_string());
    for name in ["alpha", "beta", "gamma"] {
        rx.wait_diag(&format!("{name}: opened v1"));
    }
    assert_eq!(
        hover(&manager, &file),
        ("gamma".into(), Some("gamma v1 open".into()))
    );
    // Only servers answering null: the null answer comes back.
    let stopped = manager.reconfigure(
        vec![
            config("alpha", "utf-16", "null"),
            config("gamma", "utf-16", "null"),
        ],
        languages(&["alpha", "gamma"]),
    );
    assert_eq!(stopped, vec!["beta", "gamma"]);
    rx.wait_diag("gamma: opened v1");
    assert_eq!(hover(&manager, &file).1, None);
    // Hover switched off for alpha: it is not asked at all.
    let mut alpha = config("alpha", "utf-16", "normal");
    alpha.features.hover = false;
    manager.reconfigure(
        vec![alpha, config("gamma", "utf-16", "normal")],
        languages(&["alpha", "gamma"]),
    );
    rx.wait_diag("alpha: opened v1");
    rx.wait_diag("gamma: opened v1");
    assert_eq!(hover(&manager, &file).0, "gamma");
    manager.shutdown_all();
}

/// Servers with different position encodings all sync incrementally.
fn every_encoding_gets_incremental_changes() {
    let (_dir, file) = project();
    let servers = vec![
        config("alpha", "utf-8", "normal"),
        config("beta", "utf-16", "normal"),
    ];
    let (manager, rx) = LspManager::new(servers, languages(&["alpha", "beta"]));
    let mut rx = Events {
        rx,
        seen: Vec::new(),
    };
    manager.open_document(&file, "fake", 1, "héllo".to_string());
    rx.wait_diag("alpha: opened v1");
    rx.wait_diag("beta: opened v1");
    let asked = std::cell::RefCell::new(Vec::new());
    manager.change_document_multi(
        &file,
        2,
        |encoding| {
            asked.borrow_mut().push(encoding);
            vec![TextChange {
                range: Some(Range::new(Position::new(0, 0), Position::new(0, 1))),
                text: "H".into(),
            }]
        },
        || panic!("no server needs the full text"),
    );
    assert_eq!(asked.borrow().len(), 2, "one change list per encoding");
    rx.wait_diag("alpha: changed v2 incremental");
    rx.wait_diag("beta: changed v2 incremental");
    manager.shutdown_all();
}

/// A reload restarts only servers whose configuration changed, reopens
/// documents on them with the latest version, and follows changes of a
/// language's server list. Diagnostics of a server switched off for
/// diagnostics are dropped.
fn reconfigure_touches_only_changed_servers() {
    let (_dir, file) = project();
    let servers = || {
        vec![
            config("alpha", "utf-16", "normal"),
            config("beta", "utf-16", "normal"),
        ]
    };
    let (manager, rx) = LspManager::new(servers(), languages(&["alpha", "beta"]));
    let mut rx = Events {
        rx,
        seen: Vec::new(),
    };
    manager.open_document(&file, "fake", 1, "x".to_string());
    rx.wait_diag("alpha: opened v1");
    rx.wait_diag("beta: opened v1");
    manager.change_document(&file, 2, vec![], "y".to_string());
    rx.wait_diag("alpha: changed v2 full");
    rx.wait_diag("beta: changed v2 full");

    // Same configuration: nothing happens.
    assert!(
        manager
            .reconfigure(servers(), languages(&["alpha", "beta"]))
            .is_empty()
    );
    // beta changes: only beta restarts, after the old one stopped.
    let mut changed = servers();
    changed[1].args = vec!["--changed".into()];
    let stopped = manager.reconfigure(changed.clone(), languages(&["alpha", "beta"]));
    assert_eq!(stopped, vec!["beta"]);
    rx.wait_diag("beta: opened v2");
    let order: Vec<&ServerStatus> = rx
        .seen
        .iter()
        .filter_map(|e| match e {
            LspEvent::ServerStatus { server, status, .. } if server == "beta" => Some(status),
            _ => None,
        })
        .collect();
    let stop = order.iter().position(|s| **s == ServerStatus::Stopped);
    let start = order.iter().rposition(|s| **s == ServerStatus::Starting);
    assert!(stop < start, "old instance stopped first: {order:?}");
    assert!(
        !rx.drain_diags()
            .iter()
            .any(|m| m.starts_with("alpha: opened")),
        "alpha was not reopened"
    );

    // beta leaves the language: it gets didClose and stays configured.
    assert!(
        manager
            .reconfigure(changed.clone(), languages(&["alpha"]))
            .is_empty()
    );
    rx.wait_diag("beta: cleared");
    assert_eq!(manager.running_servers(&file), vec!["alpha"]);

    // Back in, with diagnostics switched off: opened, but silent.
    changed[1].features.diagnostics = false;
    let stopped = manager.reconfigure(changed, languages(&["alpha", "beta"]));
    assert_eq!(stopped, vec!["beta"]);
    let deadline = Instant::now() + Duration::from_secs(20);
    while manager.running_servers(&file) != ["alpha", "beta"] {
        assert!(Instant::now() < deadline, "beta did not come back");
        std::thread::sleep(Duration::from_millis(10));
    }
    manager.change_document(&file, 3, vec![], "z".to_string());
    rx.wait_diag("alpha: changed v3 full");
    // beta's own hover answer proves it processed the change before.
    let answer = block_on(
        manager.request_to::<HoverRequest>(&file, "beta", |_| HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(path_to_uri(&file)),
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
        }),
    );
    assert!(answer.is_ok());
    assert!(
        !rx.drain_diags()
            .iter()
            .any(|m| m.starts_with("beta: changed")),
        "beta's diagnostics are dropped"
    );
    manager.shutdown_all();
}

/// Restarting one server reopens its documents; the other keeps running.
fn restart_server_reopens_documents() {
    let (_dir, file) = project();
    let servers = vec![
        config("alpha", "utf-16", "normal"),
        config("beta", "utf-16", "normal"),
    ];
    let (manager, rx) = LspManager::new(servers, languages(&["alpha", "beta"]));
    let mut rx = Events {
        rx,
        seen: Vec::new(),
    };
    manager.open_document(&file, "fake", 4, "x".to_string());
    rx.wait_diag("alpha: opened v4");
    rx.wait_diag("beta: opened v4");
    assert!(manager.restart_server("alpha"));
    rx.wait_diag("alpha: opened v4");
    assert!(!manager.restart_server("nobody"));
    assert_eq!(manager.servers_for_language("fake"), vec!["alpha", "beta"]);
    assert_eq!(
        manager.server_config("beta").map(|c| c.name),
        Some("beta".into())
    );
    manager.shutdown_all();
}

/// Closing the last document of an instance lets `stop_idle_servers`
/// shut it down; a server still serving a document keeps running, and
/// opening a file again starts a fresh instance.
fn idle_servers_stop_after_their_last_document_closes() {
    let (_dir, file) = project();
    let other = file.with_file_name("other.fk");
    let (manager, rx) = LspManager::new(
        vec![config("alpha", "utf-16", "normal")],
        languages(&["alpha"]),
    );
    let mut rx = Events {
        rx,
        seen: Vec::new(),
    };
    manager.open_document(&file, "fake", 1, "x".to_string());
    manager.open_document(&other, "fake", 1, "y".to_string());
    rx.wait_diag("alpha: opened v1");
    rx.wait_diag("alpha: opened v1");
    manager.close_document(&file);
    assert!(manager.stop_idle_servers().is_empty());
    assert!(manager.is_server_running("alpha"));
    manager.close_document(&other);
    assert_eq!(manager.stop_idle_servers(), vec!["alpha"]);
    assert!(!manager.is_server_running("alpha"));
    assert!(manager.server_instances().is_empty());
    rx.wait_for("alpha stopped", |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Stopped,
                ..
            }
        )
    });
    manager.open_document(&file, "fake", 2, "x".to_string());
    rx.wait_diag("alpha: opened v2");
    manager.shutdown_all();
}

fn main() {
    if std::env::var_os("ARCODE_FAKE_LSP").is_some() {
        fake_lsp::run();
        return;
    }
    let tests: [(&str, fn()); 5] = [
        (
            "single_answers_fall_back_down_the_list",
            single_answers_fall_back_down_the_list,
        ),
        (
            "every_encoding_gets_incremental_changes",
            every_encoding_gets_incremental_changes,
        ),
        (
            "reconfigure_touches_only_changed_servers",
            reconfigure_touches_only_changed_servers,
        ),
        (
            "restart_server_reopens_documents",
            restart_server_reopens_documents,
        ),
        (
            "idle_servers_stop_after_their_last_document_closes",
            idle_servers_stop_after_their_last_document_closes,
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
    println!("manager_multi tests: {ran} passed");
}
