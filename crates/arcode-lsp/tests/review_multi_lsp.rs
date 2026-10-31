//! Review tests for "several language servers per file". This binary has
//! its own `main` (harness = false): with `ARCODE_FAKE_LSP` set it acts as a
//! fake language server (see `fake_lsp`).
//!
//! Run: `cargo test -p arcode-lsp --test review_multi_lsp`

mod fake_lsp;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arcode_lsp::{LspEvent, LspManager, ServerConfig};

fn config(name: &str) -> ServerConfig {
    let exe = std::env::current_exe().expect("current exe");
    ServerConfig {
        name: name.into(),
        command: exe.to_string_lossy().into_owned(),
        env: vec![
            ("ARCODE_FAKE_LSP".into(), "1".into()),
            ("ARCODE_FAKE_NAME".into(), name.into()),
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

/// A workspace `proj/` and, outside it, a "toolchain library" `lib/` with
/// its own root marker (like `library/core/Cargo.toml` next to std sources).
fn layout() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("proj");
    let lib = dir.path().join("lib");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::create_dir_all(lib.join("src")).unwrap();
    std::fs::write(ws.join("project.marker"), "").unwrap();
    std::fs::write(lib.join("project.marker"), "").unwrap();
    (dir, ws, lib)
}

/// Library files reached through go-to-definition are attached to the
/// workspace's server instance. Restarting that server (status bar menu,
/// palette, or a config reload via `reconfigure`) must keep them there.
///
/// Bug: `reattach_all` walks the open documents in `HashMap` order after
/// the workspace instance was removed. When an attached library file comes
/// first, `shared_root` finds no running workspace instance and a *second*
/// server is started for the library's own root (for rust-analyzer: a full
/// extra instance indexing the toolchain's `library/core`).
fn restart_keeps_attached_files_on_the_workspace_instance() {
    // Several rounds: each manager has its own HashMap seed, so a lucky
    // iteration order in one round does not hide the bug.
    for round in 0..3 {
        let (_dir, ws, lib) = layout();
        let (manager, _rx) = LspManager::new(vec![config("alpha")], languages(&["alpha"]));
        manager.set_workspace_root(Some(ws.clone()));
        manager.open_document(&ws.join("src/main.fk"), "fake", 1, "x".to_string());
        for i in 0..12 {
            let file = lib.join(format!("src/f{i}.fk"));
            manager.attach_to_workspace(&file);
            manager.open_document(&file, "fake", 1, "y".to_string());
        }
        let only_ws = vec![("alpha".to_string(), ws.clone())];
        assert_eq!(manager.server_instances(), only_ws, "before restart");

        assert!(manager.restart_server("alpha"));
        assert_eq!(
            manager.server_instances(),
            only_ws,
            "round {round}: after restart_server an extra instance was started \
             for the library root"
        );

        let mut changed = config("alpha");
        changed.args = vec!["--changed".into()];
        manager.reconfigure(vec![changed], languages(&["alpha"]));
        assert_eq!(
            manager.server_instances(),
            only_ws,
            "round {round}: after reconfigure an extra instance was started \
             for the library root"
        );
        manager.shutdown_all();
    }
}

/// A server listed twice for one language (`servers = ["ra", "typos",
/// "ra"]`, an easy config mistake that nothing warns about) must still serve
/// the document once.
///
/// Bug: `attach_servers` pushes the same slot twice, so the server gets two
/// `didOpen`s, every incremental `didChange` twice (its copy of the text is
/// corrupted), and `request_all` asks it twice (every completion item
/// shows up twice, since items of one server are never deduplicated).
fn duplicate_server_names_serve_the_document_once() {
    let (_dir, ws, _lib) = layout();
    let file = ws.join("src/main.fk");
    let (manager, rx) = LspManager::new(vec![config("alpha")], languages(&["alpha", "alpha"]));
    manager.open_document(&file, "fake", 1, "x".to_string());
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut opened = 0;
    while Instant::now() < deadline && manager.running_servers(&file).is_empty() {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Give the server time to publish for every didOpen it got.
    std::thread::sleep(Duration::from_millis(500));
    while let Ok(ev) = rx.try_recv() {
        if let LspEvent::Diagnostics { diagnostics, .. } = ev
            && diagnostics.iter().any(|d| d.message == "alpha: opened v1")
        {
            opened += 1;
        }
    }
    assert_eq!(
        manager.running_servers(&file),
        vec!["alpha"],
        "the document is attached to the same server twice"
    );
    assert_eq!(opened, 1, "didOpen was sent {opened} times");
    manager.shutdown_all();
}

/// Only library files are open (the workspace file that led there was
/// closed): a restart must still bring the server back at the workspace
/// root, not start one for the library's own root.
fn restart_with_only_library_files_open() {
    let (_dir, ws, lib) = layout();
    let (manager, _rx) = LspManager::new(vec![config("alpha")], languages(&["alpha"]));
    manager.set_workspace_root(Some(ws.clone()));
    let main = ws.join("src/main.fk");
    manager.open_document(&main, "fake", 1, "x".to_string());
    let file = lib.join("src/f.fk");
    manager.attach_to_workspace(&file);
    manager.open_document(&file, "fake", 1, "y".to_string());
    manager.close_document(&main);
    assert!(manager.restart_server("alpha"));
    assert_eq!(
        manager.server_instances(),
        vec![("alpha".to_string(), ws.clone())]
    );
    manager.shutdown_all();
}

/// Servers answer "not found" with `[]` as often as with `null`: an empty
/// definition list must fall back to the next server too.
fn empty_definition_falls_back() {
    use arcode_lsp::lsp_types::request::GotoDefinition;
    use arcode_lsp::lsp_types::{
        GotoDefinitionParams, GotoDefinitionResponse, Position, TextDocumentIdentifier,
        TextDocumentPositionParams,
    };
    let (_dir, ws, _lib) = layout();
    let file = ws.join("src/main.fk");
    let with_definition = |name: &str, mode: &str| {
        let mut c = config(name);
        c.env.push(("ARCODE_FAKE_DEFINITION".into(), mode.into()));
        c
    };
    let (manager, _rx) = LspManager::new(
        vec![
            with_definition("alpha", "empty"),
            with_definition("beta", "here"),
        ],
        languages(&["alpha", "beta"]),
    );
    manager.open_document(&file, "fake", 1, "x".to_string());
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && manager.running_servers(&file).len() < 2 {
        std::thread::sleep(Duration::from_millis(10));
    }
    let request = manager.request_with::<GotoDefinition>(
        &file,
        |c| c.definition_provider.is_some(),
        |_| GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: arcode_lsp::path_to_uri(&file),
                },
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server, answer) = runtime.block_on(request).expect("an answer");
    assert_eq!(server, "beta", "alpha's empty list stopped the fallback");
    let Some(GotoDefinitionResponse::Array(locations)) = answer else {
        panic!("unexpected answer {answer:?}");
    };
    assert_eq!(locations[0].range.start.character, 7);
    manager.shutdown_all();
}

fn main() {
    if std::env::var_os("ARCODE_FAKE_LSP").is_some() {
        fake_lsp::run();
        return;
    }
    let tests: [(&str, fn()); 4] = [
        (
            "restart_keeps_attached_files_on_the_workspace_instance",
            restart_keeps_attached_files_on_the_workspace_instance,
        ),
        (
            "duplicate_server_names_serve_the_document_once",
            duplicate_server_names_serve_the_document_once,
        ),
        (
            "restart_with_only_library_files_open",
            restart_with_only_library_files_open,
        ),
        ("empty_definition_falls_back", empty_definition_falls_back),
    ];
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let mut failed = Vec::new();
    for (name, test) in tests {
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        println!("test {name} ...");
        match std::panic::catch_unwind(test) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.push(name);
            }
        }
    }
    if !failed.is_empty() {
        eprintln!("failed: {failed:?}");
        std::process::exit(1);
    }
}
