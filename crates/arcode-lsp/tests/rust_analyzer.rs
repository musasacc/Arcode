//! Runs the real rust-analyzer. Ignored by default:
//! `cargo test -p arcode-lsp --test rust_analyzer -- --ignored`

use std::collections::HashMap;
use std::time::{Duration, Instant};

use arcode_lsp::lsp_types::{DiagnosticSeverity, MessageType};
use arcode_lsp::{FileChange, LspEvent, LspManager, ServerConfig, ServerStatus};

#[test]
#[ignore = "needs rust-analyzer and cargo on PATH; slow"]
fn rust_analyzer_reports_type_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ra_probe");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ra_probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    let source = "fn main() {\n    let x: i32 = \"not a number\";\n    println!(\"{x}\");\n}\n";
    let main = root.join("src/main.rs");
    std::fs::write(&main, source).unwrap();

    let config = ServerConfig::new("rust-analyzer", "rust-analyzer", &["Cargo.toml", ".git"]);
    let languages = HashMap::from([("rust".to_string(), vec!["rust-analyzer".to_string()])]);
    let (manager, rx) = LspManager::new(vec![config], languages);
    manager.open_document(&main, "rust", 1, source.to_string());

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut found = None;
    while Instant::now() < deadline && found.is_none() {
        let ev = match rx.try_recv() {
            Ok(ev) => ev,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
        };
        match ev {
            LspEvent::Diagnostics {
                path, diagnostics, ..
            } if path == main => {
                found = diagnostics
                    .into_iter()
                    .find(|d| d.severity == Some(DiagnosticSeverity::ERROR));
            }
            LspEvent::ServerStatus {
                status: ServerStatus::Failed(msg),
                ..
            } => {
                panic!("rust-analyzer failed: {msg}")
            }
            LspEvent::ServerStatus { status, .. } => eprintln!("status: {status:?}"),
            LspEvent::Progress {
                title,
                message,
                done,
                ..
            } => {
                eprintln!("progress: {title:?} {message:?} done={done}")
            }
            _ => {}
        }
    }
    let diag = found.expect("no error diagnostic from rust-analyzer within 90s");
    eprintln!("diagnostic: {} ({:?})", diag.message, diag.source);
    assert_eq!(diag.range.start.line, 1);
    manager.shutdown_all();
}

/// An error or a warning, which Arcode shows as a notification.
fn serious(typ: MessageType) -> bool {
    typ == MessageType::ERROR || typ == MessageType::WARNING
}

/// Hover text at `line`/`character` of `path`, or `None`.
fn hover_text(
    manager: &LspManager,
    path: &std::path::Path,
    line: u32,
    character: u32,
) -> Option<String> {
    use arcode_lsp::lsp_types::request::HoverRequest;
    use arcode_lsp::lsp_types::{
        HoverContents, HoverParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
    };
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: arcode_lsp::path_to_uri(path),
            },
            position: Position::new(line, character),
        },
        work_done_progress_params: Default::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (_, hover) = runtime
        .block_on(manager.request::<HoverRequest>(path, params, |_| true))
        .ok()?;
    match hover?.contents {
        HoverContents::Markup(m) => Some(m.value),
        other => Some(format!("{other:?}")),
    }
}

/// Hover text at `line`/`character` of `path` once there is one (the
/// standard library may still be loading), within 60 seconds.
fn wait_hover(manager: &LspManager, path: &std::path::Path, line: u32, character: u32) -> String {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(text) = hover_text(manager, path, line, character) {
            return text;
        }
        assert!(Instant::now() < deadline, "no hover in {path:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// A Rust file outside any Cargo project gets completion-grade analysis
/// (hover on standard library items, type errors) without a "Failed to
/// discover workspace" message; a second loose file in the folder joins,
/// and a renamed one is followed.
#[test]
#[ignore = "needs rust-analyzer and the rust-src component; slow"]
fn rust_analyzer_serves_loose_files() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let test = scratch.join("test.rs");
    let source = "fn main() {\n    let s = String::from(\"hi\");\n    let x: i32 = \"no\";\n}\n";
    std::fs::write(&test, source).unwrap();
    let other = scratch.join("other.rs");
    let other_source = "fn helper() -> usize {\n    let s = String::new();\n    s.len()\n}\n";
    std::fs::write(&other, other_source).unwrap();

    let config = ServerConfig::new("rust-analyzer", "rust-analyzer", &["Cargo.toml"]);
    let languages = HashMap::from([("rust".to_string(), vec!["rust-analyzer".to_string()])]);
    let (manager, rx) = LspManager::new(vec![config], languages);
    manager.open_document(&test, "rust", 1, source.to_string());

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut error = None;
    while Instant::now() < deadline && error.is_none() {
        match rx.try_recv() {
            Ok(LspEvent::Diagnostics {
                path, diagnostics, ..
            }) if path == test => {
                error = diagnostics
                    .into_iter()
                    .find(|d| d.severity == Some(DiagnosticSeverity::ERROR));
            }
            Ok(LspEvent::ShowMessage { typ, message, .. }) if serious(typ) => {
                panic!("message shown for a loose file ({typ:?}): {message}")
            }
            Ok(LspEvent::ServerStatus {
                status: ServerStatus::Failed(msg),
                ..
            }) => panic!("rust-analyzer failed: {msg}"),
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let error = error.expect("no type error from rust-analyzer within 90s");
    assert_eq!(error.range.start.line, 2);
    let hover = wait_hover(&manager, &test, 1, 14);
    assert!(hover.contains("pub struct String"), "{hover}");

    // Served once the reload with the second file finished.
    manager.open_document(&other, "rust", 1, other_source.to_string());
    let hover = wait_hover(&manager, &other, 2, 7);
    assert!(hover.contains("fn len(&self) -> usize"), "{hover}");
    while let Ok(event) = rx.try_recv() {
        if let LspEvent::ShowMessage { typ, message, .. } = event
            && serious(typ)
        {
            panic!("message shown for a loose file ({typ:?}): {message}");
        }
    }
    assert_eq!(manager.server_instances().len(), 1);

    // Renamed as the file tree does it, with the other file closed. With
    // the old path left in its list rust-analyzer would rebuild its
    // workspace without end and serve nothing; the instance restarts with
    // the new one. (Its diagnostics are not checked: rust-analyzer 1.97
    // does not always report them for a detached file.)
    manager.close_document(&other);
    let renamed = scratch.join("renamed.rs");
    std::fs::rename(&test, &renamed).unwrap();
    manager.close_document(&test);
    manager.open_document(&renamed, "rust", 1, source.to_string());
    manager.files_changed(&[FileChange::Renamed(test.clone(), renamed.clone())]);
    let hover = wait_hover(&manager, &renamed, 1, 14);
    assert!(hover.contains("pub struct String"), "{hover}");
    std::thread::sleep(Duration::from_secs(3));
    while rx.try_recv().is_ok() {}
    let busy = progress_reports(&rx, Duration::from_secs(5));
    assert!(
        busy < 5,
        "still busy after the rename: {busy} progress reports"
    );
    manager.shutdown_all();
}

/// How many work-done progress reports arrive within `window`.
fn progress_reports(rx: &arcode_lsp::EventReceiver, window: Duration) -> usize {
    let end = Instant::now() + window;
    let mut count = 0;
    while Instant::now() < end {
        match rx.try_recv() {
            Ok(LspEvent::Progress { .. }) => count += 1,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    count
}
