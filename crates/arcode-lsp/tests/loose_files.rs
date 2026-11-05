//! Rust files outside any Cargo project: rust-analyzer gets them as
//! detached files, and its list follows renames and deletes. This binary
//! has its own `main` (harness = false): with `ARCODE_FAKE_LSP` set it acts
//! as a fake language server (see `fake_lsp`).

mod fake_lsp;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use arcode_lsp::lsp_types::request::HoverRequest;
use arcode_lsp::lsp_types::{
    HoverContents, HoverParams, MessageType, Position, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use arcode_lsp::{
    EventReceiver, FileChange, LspEvent, LspManager, ServerConfig, ServerStatus, path_to_uri,
};
use serde_json::{Value, json};

/// The fake server, named and configured like rust-analyzer; it answers
/// hover with its current settings.
fn rust_analyzer() -> ServerConfig {
    let exe = std::env::current_exe().expect("current exe");
    ServerConfig {
        name: "rust-analyzer".into(),
        command: exe.to_string_lossy().into_owned(),
        env: vec![
            ("ARCODE_FAKE_LSP".into(), "1".into()),
            ("ARCODE_FAKE_NAME".into(), "rust-analyzer".into()),
            ("ARCODE_FAKE_ENCODING".into(), "utf-16".into()),
            ("ARCODE_FAKE_HOVER".into(), "options".into()),
        ],
        root_markers: vec!["Cargo.toml".into()],
        ..Default::default()
    }
}

fn manager() -> (LspManager, EventReceiver) {
    let languages = HashMap::from([("rust".to_string(), vec!["rust-analyzer".to_string()])]);
    LspManager::new(vec![rust_analyzer()], languages)
}

/// A temporary folder, unless it sits inside a Cargo project (its files
/// would not be loose).
fn scratch() -> Option<tempfile::TempDir> {
    let dir = tempfile::tempdir().unwrap();
    let inside = dir
        .path()
        .ancestors()
        .any(|d| d.join("Cargo.toml").exists());
    if inside {
        println!("skipped: the temporary folder is inside a Cargo project");
        return None;
    }
    Some(dir)
}

/// Writes Rust files `names` into `dir`.
fn write(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    names
        .iter()
        .map(|n| {
            std::fs::write(dir.join(n), "fn main() {}\n").unwrap();
            dir.join(n)
        })
        .collect()
}

fn open(manager: &LspManager, path: &Path, version: i32) {
    manager.open_document(path, "rust", version, "fn main() {}\n".to_string());
}

/// Waits up to 20 seconds for an event `what` describes.
fn wait_for(rx: &EventReceiver, what: &str, mut found: impl FnMut(&LspEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "no {what}");
        match rx.try_recv() {
            Ok(event) if found(&event) => return,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Waits for a diagnostic message from the fake server on `path`.
fn wait_message(rx: &EventReceiver, path: &Path, message: &str) {
    wait_for(rx, &format!("{message:?} on {path:?}"), |event| {
        matches!(event, LspEvent::Diagnostics { path: p, diagnostics, .. }
            if p == path && diagnostics.iter().any(|d| d.message.ends_with(message)))
    });
}

/// The settings the fake server has for `path`'s instance.
fn server_settings(manager: &LspManager, path: &Path) -> Value {
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: path_to_uri(path),
            },
            position: Position::new(0, 0),
        },
        work_done_progress_params: Default::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let request = manager.request::<HoverRequest>(path, params, |_| true);
    let (_, hover) = runtime.block_on(request).expect("hover");
    match hover.expect("hover answer").contents {
        HoverContents::Markup(m) => serde_json::from_str(&m.value).unwrap(),
        other => panic!("unexpected hover {other:?}"),
    }
}

fn detached(files: &[&PathBuf]) -> Value {
    json!({ "detachedFiles": files, "checkOnSave": false })
}

/// Rust files outside any Cargo project share one rust-analyzer instance
/// per folder, which gets them as detached files: the first one in its
/// initialization options, later ones through a settings change followed
/// by a workspace reload (after their `didOpen`). Files of a Cargo project
/// keep the normal path.
fn loose_rust_files_are_detached_files() {
    let Some(dir) = scratch() else { return };
    let scratch = dir.path().join("scratch");
    let cargo = dir.path().join("proj");
    let [a, b] = write(&scratch, &["a.rs", "b.rs"]).try_into().unwrap();
    let [main] = write(&cargo.join("src"), &["main.rs"]).try_into().unwrap();
    std::fs::write(cargo.join("Cargo.toml"), "").unwrap();
    let (manager, rx) = manager();

    open(&manager, &a, 1);
    wait_message(&rx, &a, "opened v1");
    assert_eq!(server_settings(&manager, &a), detached(&[&a]));
    open(&manager, &b, 1);
    // The reload comes after the server asked for the longer list.
    wait_message(&rx, &b, "reloaded a.rs,b.rs");
    assert_eq!(server_settings(&manager, &b), detached(&[&a, &b]));
    // Reopening a known file sends nothing new.
    open(&manager, &a, 2);
    wait_message(&rx, &a, "opened v2");
    assert_eq!(
        manager.server_instances(),
        vec![("rust-analyzer".into(), scratch.clone())]
    );

    open(&manager, &main, 1);
    wait_message(&rx, &main, "opened v1");
    assert_eq!(server_settings(&manager, &main), Value::Null);
    assert_eq!(
        manager.server_instances(),
        vec![
            ("rust-analyzer".into(), cargo.clone()),
            ("rust-analyzer".into(), scratch.clone())
        ]
    );
    manager.shutdown_all();
}

/// The file names in the `detachedFiles` the fake server has for `path`,
/// sorted (a restart reopens documents in no particular order).
fn detached_names(manager: &LspManager, path: &Path) -> Vec<String> {
    let settings = server_settings(manager, path);
    let mut names: Vec<String> = settings["detachedFiles"]
        .as_array()
        .expect("detachedFiles")
        .iter()
        .map(|f| {
            f.as_str()
                .unwrap()
                .rsplit(['/', '\\'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

/// A loose file renamed or deleted in Arcode leaves the instance's list
/// (rust-analyzer rebuilds forever while the list names a missing file):
/// the instance restarts with the open files that exist, or stops when
/// none is left, and the next loose file starts a new one.
fn renamed_and_deleted_files_leave_the_list() {
    let Some(dir) = scratch() else { return };
    let [a, b] = write(dir.path(), &["a.rs", "b.rs"]).try_into().unwrap();
    let (manager, rx) = manager();
    open(&manager, &a, 1);
    wait_message(&rx, &a, "opened v1");
    open(&manager, &b, 1);
    wait_message(&rx, &b, "reloaded a.rs,b.rs");

    // As the file tree does it: the file moves, its editor follows, then
    // the servers hear about the change. The restarted instance opens the
    // documents again.
    let c = dir.path().join("c.rs");
    std::fs::rename(&a, &c).unwrap();
    manager.close_document(&a);
    open(&manager, &c, 2);
    manager.files_changed(&[FileChange::Renamed(a.clone(), c.clone())]);
    wait_message(&rx, &c, "opened v2");
    assert_eq!(detached_names(&manager, &c), ["b.rs", "c.rs"]);

    // Deleted with its tab closed.
    std::fs::remove_file(&b).unwrap();
    manager.close_document(&b);
    manager.files_changed(&[FileChange::Deleted(b.clone())]);
    wait_for(&rx, "restart", |event| {
        matches!(
            event,
            LspEvent::ServerStatus {
                status: ServerStatus::Stopped,
                ..
            }
        )
    });
    wait_message(&rx, &c, "opened v2");
    assert_eq!(detached_names(&manager, &c), ["c.rs"]);

    // The last one deleted while an unsaved buffer keeps it open.
    std::fs::remove_file(&c).unwrap();
    manager.files_changed(&[FileChange::Deleted(c.clone())]);
    wait_for(&rx, "stop", |event| {
        matches!(
            event,
            LspEvent::ServerStatus {
                status: ServerStatus::Stopped,
                ..
            }
        )
    });
    assert_eq!(manager.server_instances(), vec![]);

    let [d] = write(dir.path(), &["d.rs"]).try_into().unwrap();
    open(&manager, &d, 1);
    wait_message(&rx, &d, "opened v1");
    assert_eq!(server_settings(&manager, &d), detached(&[&d]));
    manager.shutdown_all();
}

/// A loose file deleted outside Arcode leaves the list once a project
/// watcher reports changes on disk.
fn files_deleted_on_disk_leave_the_list() {
    let Some(dir) = scratch() else { return };
    let [a, b] = write(dir.path(), &["a.rs", "b.rs"]).try_into().unwrap();
    let (manager, rx) = manager();
    open(&manager, &a, 1);
    wait_message(&rx, &a, "opened v1");
    open(&manager, &b, 1);
    wait_message(&rx, &b, "reloaded a.rs,b.rs");
    // Nothing changes while every file exists.
    manager.files_changed_on_disk();
    std::fs::remove_file(&b).unwrap();
    manager.files_changed_on_disk();
    wait_for(&rx, "restart", |event| {
        matches!(
            event,
            LspEvent::ServerStatus {
                status: ServerStatus::Stopped,
                ..
            }
        )
    });
    wait_message(&rx, &a, "opened v1");
    assert_eq!(detached_names(&manager, &a), ["a.rs"]);
    manager.shutdown_all();
}

/// rust-analyzer would read every Rust file under a loose file's folder:
/// a folder with too many is refused with a message, and nothing starts.
fn folders_too_big_are_refused() {
    let Some(dir) = scratch() else { return };
    let names: Vec<String> = (0..201).map(|i| format!("f{i}.rs")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let files = write(dir.path(), &names);
    let (manager, rx) = manager();
    // Low-end mode allows 200 Rust files.
    manager.set_low_end_mode(true);
    open(&manager, &files[0], 1);
    wait_for(&rx, "message", |event| match event {
        LspEvent::ShowMessage { typ, message, .. } => {
            *typ == MessageType::WARNING && message.contains(&*dir.path().to_string_lossy())
        }
        LspEvent::ServerStatus { status, .. } => panic!("server status {status:?}"),
        _ => false,
    });
    assert_eq!(manager.server_instances(), vec![]);
    // In normal mode the folder is small enough.
    manager.set_low_end_mode(false);
    open(&manager, &files[1], 1);
    wait_message(&rx, &files[1], "opened v1");
    assert_eq!(server_settings(&manager, &files[1]), detached(&[&files[1]]));
    manager.shutdown_all();
}

fn main() {
    if std::env::var_os("ARCODE_FAKE_LSP").is_some() {
        fake_lsp::run();
        return;
    }
    let tests: [(&str, fn()); 4] = [
        (
            "loose_rust_files_are_detached_files",
            loose_rust_files_are_detached_files,
        ),
        (
            "renamed_and_deleted_files_leave_the_list",
            renamed_and_deleted_files_leave_the_list,
        ),
        (
            "files_deleted_on_disk_leave_the_list",
            files_deleted_on_disk_leave_the_list,
        ),
        ("folders_too_big_are_refused", folders_too_big_are_refused),
    ];
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    for (name, test) in tests {
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        println!("test {name} ...");
        test();
        println!("test {name} ... ok");
    }
}
