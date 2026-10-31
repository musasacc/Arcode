//! Runs the real rust-analyzer. Ignored by default:
//! `cargo test -p arcode-lsp --test rust_analyzer -- --ignored`

use std::collections::HashMap;
use std::time::{Duration, Instant};

use arcode_lsp::lsp_types::DiagnosticSeverity;
use arcode_lsp::{LspEvent, LspManager, ServerConfig, ServerStatus};

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
