//! Diagnostics, Problems and status bar wiring, driven by injected
//! language server events (no real server runs in tests).

use std::fs;

use arcode_buffer::Point;
use arcode_editor::DiagnosticSeverity;
use arcode_lsp::lsp_types::{self, Diagnostic, Position, Range};
use arcode_lsp::{LspEvent, ServerStatus};
use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::problems::ProblemRow;

fn type_error() -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(1, 17), Position::new(1, 24)),
        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
        source: Some("rustc".into()),
        message: "mismatched types\nexpected `i32`, found `&str`".into(),
        ..Default::default()
    }
}

#[gpui::test]
fn diagnostics_reach_editor_problems_and_status(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let main_rs = root.join("src/main.rs");
    fs::write(&main_rs, "fn main() {\n    let x: i32 = \"hello\";\n}\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main_rs, window, cx);
    });
    cx.run_until_parked();

    let lsp = ws.read_with(cx, |ws, _| ws.lsp().clone());
    lsp.update(cx, |lsp, cx| {
        let status = |status| LspEvent::ServerStatus {
            server: "rust-analyzer".into(),
            root: root.clone(),
            status,
        };
        lsp.handle_event(status(ServerStatus::Starting), cx);
        lsp.handle_event(status(ServerStatus::Running), cx);
        lsp.handle_event(
            LspEvent::Progress {
                server: "rust-analyzer".into(),
                token: "rustAnalyzer/Indexing".into(),
                title: Some("Indexing".into()),
                message: None,
                percentage: Some(40),
                done: false,
            },
            cx,
        );
        lsp.handle_event(
            LspEvent::Diagnostics {
                server: "rust-analyzer".into(),
                path: main_rs.clone(),
                version: None,
                diagnostics: vec![type_error()],
            },
            cx,
        );
    });
    cx.run_until_parked();

    ws.update(cx, |ws, cx| {
        let info = ws.status_bar().read(cx).info().clone();
        assert_eq!(info.errors, 1);
        assert_eq!(info.warnings, 0);
        assert_eq!(
            info.lsp_status.as_ref().map(|s| s.as_ref()),
            Some("rust-analyzer: indexing 40%")
        );
        let editor = ws.editor_for_path(&main_rs, cx).expect("editor");
        let diagnostics = editor.read(cx).diagnostics().to_vec();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range, Point::new(1, 17)..Point::new(1, 24));
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].source, "rustc");
        let problems = ws.bottom_panel().read(cx).problems().clone();
        let problems = problems.read(cx);
        assert_eq!(problems.count(), 1);
        assert!(matches!(
            &problems.rows()[0],
            ProblemRow::File { label, count: 1, .. } if label == "src/main.rs"
        ));
        assert!(matches!(
            &problems.rows()[1],
            ProblemRow::Problem { message, .. } if message.as_ref() == "mismatched types"
        ));
    });

    // Switch to another file, then pick the problem: main.rs comes back
    // with the cursor on the error.
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/lib.rs"), window, cx)
    });
    let problems = ws.read_with(cx, |ws, cx| ws.bottom_panel().read(cx).problems().clone());
    problems.update(cx, |p, cx| p.open_row(1, cx));
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        let editor = ws.active_editor(cx).expect("active editor");
        let e = editor.read(cx);
        assert_eq!(e.path(), Some(main_rs.as_path()));
        assert_eq!(e.cursor_position(), Point::new(1, 17));
        let info = ws.status_bar().read(cx).info().clone();
        assert_eq!(info.cursor, Some((1, 17)));
    });

    // The server clears its diagnostics after the fix.
    lsp.update(cx, |lsp, cx| {
        lsp.handle_event(
            LspEvent::Diagnostics {
                server: "rust-analyzer".into(),
                path: main_rs.clone(),
                version: None,
                diagnostics: Vec::new(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.status_bar().read(cx).info().errors, 0);
        let editor = ws.editor_for_path(&main_rs, cx).expect("editor");
        assert!(editor.read(cx).diagnostics().is_empty());
        assert_eq!(ws.bottom_panel().read(cx).problems().read(cx).count(), 0);
    });

    // Closing the project forgets diagnostics and server state.
    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        assert!(ws.lsp().read(cx).store().is_empty());
        assert_eq!(ws.status_bar().read(cx).info().lsp_status, None);
    });
}
