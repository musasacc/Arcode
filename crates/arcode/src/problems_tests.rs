//! Tests of the Problems list.

use super::*;
use arcode_lsp::lsp_types::{Diagnostic, DiagnosticSeverity as Sev, Range};

fn diag(line: u32, sev: Sev, msg: &str) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(line, 4), Position::new(line, 9)),
        severity: Some(sev),
        source: Some("rustc".into()),
        message: msg.into(),
        ..Default::default()
    }
}

#[test]
fn file_labels_split_into_folder_and_name() {
    assert_eq!(split_label("src/main.rs"), ("src", "main.rs"));
    assert_eq!(split_label("a/b/c.rs"), ("a/b", "c.rs"));
    assert_eq!(split_label("Cargo.toml"), ("", "Cargo.toml"));
}

#[test]
fn only_errors_and_warnings_count() {
    assert!(counts_as_problem(DiagnosticSeverity::Error));
    assert!(counts_as_problem(DiagnosticSeverity::Warning));
    assert!(!counts_as_problem(DiagnosticSeverity::Info));
    assert!(!counts_as_problem(DiagnosticSeverity::Hint));
}

#[test]
fn rows_are_grouped_by_file() {
    let mut store = DiagnosticStore::new();
    let root = PathBuf::from("/p");
    store.update(
        "ra",
        root.join("src/main.rs"),
        vec![
            diag(3, Sev::WARNING, "unused\nmore detail"),
            diag(1, Sev::ERROR, "mismatched types"),
        ],
    );
    store.update("ra", root.join("src/a.rs"), vec![diag(0, Sev::HINT, "h")]);
    let rows = build_rows(&store, Some(&root));
    assert_eq!(rows.len(), 5);
    // Hints are listed but not counted (like the tab badge).
    assert!(matches!(&rows[0], ProblemRow::File { label, count: 0, .. } if label == "src/a.rs"));
    assert!(matches!(&rows[2], ProblemRow::File { label, count: 2, .. } if label == "src/main.rs"));
    match &rows[3] {
        ProblemRow::Problem {
            severity,
            message,
            source,
            position,
            ..
        } => {
            assert_eq!(*severity, DiagnosticSeverity::Error);
            assert_eq!(message.as_ref(), "mismatched types");
            assert_eq!(source.as_ref(), "rustc");
            assert_eq!(*position, Position::new(1, 4));
        }
        other => panic!("unexpected row {other:?}"),
    }
    assert!(
        matches!(&rows[4], ProblemRow::Problem { message, .. } if message.as_ref() == "unused")
    );
    assert!(build_rows(&DiagnosticStore::new(), None).is_empty());
}

#[test]
fn problems_sort_by_severity_then_position() {
    let mut store = DiagnosticStore::new();
    let path = PathBuf::from("/p/a.rs");
    store.update(
        "ra",
        path.clone(),
        vec![
            diag(1, Sev::HINT, "hint"),
            diag(2, Sev::WARNING, "warn late"),
            diag(9, Sev::ERROR, "error"),
            diag(0, Sev::WARNING, "warn early"),
        ],
    );
    let messages: Vec<String> = build_rows(&store, None)
        .iter()
        .filter_map(|r| match r {
            ProblemRow::Problem { message, .. } => Some(message.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, ["error", "warn early", "warn late", "hint"]);
}
