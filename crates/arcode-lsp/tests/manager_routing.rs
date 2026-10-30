//! Which server instance a file outside the workspace joins. This binary
//! has its own `main` (harness = false): with `ARCODE_FAKE_LSP` set it acts
//! as a fake language server (see `manager.rs`).

mod fake_lsp;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use arcode_lsp::{LspEvent, LspManager, ServerConfig, ServerStatus};

fn fake_config() -> ServerConfig {
    let exe = std::env::current_exe().expect("current exe");
    ServerConfig {
        name: "alpha".into(),
        command: exe.to_string_lossy().into_owned(),
        args: vec![],
        env: vec![
            ("ARCODE_FAKE_LSP".into(), "1".into()),
            ("ARCODE_FAKE_NAME".into(), "alpha".into()),
            ("ARCODE_FAKE_ENCODING".into(), "utf-16".into()),
        ],
        root_markers: vec!["project.marker".into()],
        initialization_options: None,
    }
}

fn project(dir: &Path, name: &str) -> std::path::PathBuf {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("project.marker"), "").unwrap();
    root
}

/// A file from another project opened while a workspace is open gets its
/// own server; only files navigation attached join the workspace's.
fn only_attached_files_join_the_workspace_server() {
    let dir = tempfile::tempdir().unwrap();
    let (ws, other, lib) = (
        project(dir.path(), "ws"),
        project(dir.path(), "other"),
        project(dir.path(), "toolchain/core"),
    );
    let languages = HashMap::from([("fake".to_string(), vec!["alpha".to_string()])]);
    let (manager, _rx) = LspManager::new(vec![fake_config()], languages);
    manager.set_workspace_root(Some(ws.clone()));
    manager.open_document(&ws.join("main.fk"), "fake", 1, "x".to_string());
    manager.attach_to_workspace(&lib.join("option.fk"));
    manager.open_document(&lib.join("option.fk"), "fake", 1, "x".to_string());
    assert_eq!(manager.server_instances(), vec![("alpha".into(), ws.clone())]);
    manager.open_document(&other.join("lib.fk"), "fake", 1, "x".to_string());
    assert_eq!(
        manager.server_instances(),
        vec![("alpha".into(), other.clone()), ("alpha".into(), ws.clone())]
    );
    manager.shutdown_all();
}

/// An attached file is not sent to a workspace server that failed.
fn attached_files_skip_a_failed_server() {
    let dir = tempfile::tempdir().unwrap();
    let (ws, lib) = (project(dir.path(), "ws"), project(dir.path(), "lib"));
    let mut config = fake_config();
    config.command = "arcode-definitely-missing-server-xyz".into();
    let languages = HashMap::from([("fake".to_string(), vec!["alpha".to_string()])]);
    let (manager, rx) = LspManager::new(vec![config], languages);
    manager.set_workspace_root(Some(ws.clone()));
    manager.open_document(&ws.join("main.fk"), "fake", 1, "x".to_string());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "no Failed status");
        match rx.try_recv() {
            Ok(LspEvent::ServerStatus {
                status: ServerStatus::Failed(_),
                ..
            }) => break,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // The failure is reported just before the slot is marked stopped:
    // re-open (which routes again) until the file gets its own instance.
    manager.attach_to_workspace(&lib.join("a.fk"));
    let expected = vec![("alpha".into(), lib.clone()), ("alpha".into(), ws.clone())];
    loop {
        manager.open_document(&lib.join("a.fk"), "fake", 1, "x".to_string());
        if manager.server_instances() == expected {
            break;
        }
        assert!(Instant::now() < deadline, "the failed server was reused");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    if std::env::var_os("ARCODE_FAKE_LSP").is_some() {
        fake_lsp::run();
        return;
    }
    let tests: [(&str, fn()); 2] = [
        (
            "only_attached_files_join_the_workspace_server",
            only_attached_files_join_the_workspace_server,
        ),
        (
            "attached_files_skip_a_failed_server",
            attached_files_skip_a_failed_server,
        ),
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
