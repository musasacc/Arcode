//! GPUI tests of the status bar pickers (language, line ending,
//! indentation), file associations and the package banner.

use std::fs;
use std::path::{Path, PathBuf};

use arcode_buffer::LineEnding;
use arcode_config::Config;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::{open, project_dir};
use crate::languages::actions::{ChangeIndentation, ChangeLineEnding, SelectLanguageMode};
use crate::test_support::settle;

fn pick(ws: &Entity<Workspace>, query: &str, cx: &mut VisualTestContext) {
    let picker = ws.read_with(cx, |ws, _| ws.list_picker().cloned().expect("picker open"));
    picker.update(cx, |p, cx| {
        p.set_query(query, cx);
        p.confirm(cx);
    });
    cx.run_until_parked();
}

fn status(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> (Option<String>, String, String) {
    ws.read_with(cx, |ws, cx| {
        let bar = ws.status_bar().read(cx);
        let doc = bar.doc_status().cloned().expect("doc status");
        (
            bar.info().language.as_ref().map(|l| l.to_string()),
            doc.indent.to_string(),
            doc.line_ending.to_string(),
        )
    })
}

fn open_file(ws: &Entity<Workspace>, root: &Path, path: &Path, cx: &mut VisualTestContext) {
    let (root, path) = (root.to_path_buf(), path.to_path_buf());
    ws.update_in(cx, |ws, window, cx| {
        if ws.project().is_none() {
            ws.open_project(root, window, cx);
        }
        ws.open_file(&path, window, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn language_picker_sets_and_resets_the_language(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    open_file(&ws, &root, &root.join("src/main.rs"), cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("Rust"));

    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "plain text", cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("Plain Text"));
    assert!(ws.read_with(cx, |ws, _| ws.list_picker().is_none()));

    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "auto detect", cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("Rust"));
}

#[gpui::test]
fn associations_apply_on_open_and_on_settings_change(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("page.tpl"), "<p>hi</p>\n").expect("write");
    let (ws, cx) = open(cx);
    let set = |pattern: &str, language: &str, cx: &mut VisualTestContext| {
        let (pattern, language) = (pattern.to_string(), language.to_string());
        cx.update(|_, cx| {
            let mut config = Config::default();
            config.files.associations.insert(pattern, language);
            arcode_ui_kit::set_settings(config, cx);
        });
        cx.run_until_parked();
    };
    set("*.tpl", "html", cx);
    open_file(&ws, &root, &root.join("page.tpl"), cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("HTML"));
    set("*.tpl", "Markdown", cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("Markdown"));

    // A language picked by hand is kept when the settings change.
    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "yaml", cx);
    set("*.tpl", "html", cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("YAML"));
}

#[gpui::test]
fn line_ending_picker_changes_what_is_saved(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    open_file(&ws, &root, &root.join("src/main.rs"), cx);
    assert_eq!(status(&ws, cx).2, "LF");
    cx.dispatch_action(ChangeLineEnding);
    cx.run_until_parked();
    pick(&ws, "crlf", cx);
    assert_eq!(status(&ws, cx).2, "CRLF");
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    editor.update(cx, |e, cx| {
        assert_eq!(e.line_ending(), LineEnding::CrLf);
        assert!(e.is_dirty());
        e.save(cx).expect("save");
    });
    let text = fs::read_to_string(root.join("src/main.rs")).expect("read");
    assert_eq!(text, "fn main() {}\r\n");
}

#[gpui::test]
fn indentation_is_detected_picked_and_converted(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let file = root.join("src/tabs.rs");
    fs::write(&file, "fn a() {\n\tif x {\n\t\ty();\n\t}\n}\n").expect("write");
    let (ws, cx) = open(cx);
    open_file(&ws, &root, &file, cx);
    assert_eq!(status(&ws, cx).1, "Tab size 4");

    cx.dispatch_action(ChangeIndentation);
    cx.run_until_parked();
    pick(&ws, "convert to spaces", cx);
    assert_eq!(status(&ws, cx).1, "Spaces 4");
    let text = ws.read_with(cx, |ws, cx| {
        ws.active_editor(cx)
            .expect("editor")
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    assert_eq!(text, "fn a() {\n    if x {\n        y();\n    }\n}\n");

    cx.dispatch_action(ChangeIndentation);
    cx.run_until_parked();
    pick(&ws, "spaces: 2", cx);
    assert_eq!(status(&ws, cx).1, "Spaces 2");
    cx.dispatch_action(ChangeIndentation);
    cx.run_until_parked();
    pick(&ws, "language settings", cx);
    assert_eq!(status(&ws, cx).1, "Spaces 4");
}

fn node_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join(".git")).expect("git");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"left-pad":"1"}}"#,
    )
    .expect("write");
    fs::write(root.join("index.js"), "console.log(1)\n").expect("write");
    (dir, root)
}

#[gpui::test]
fn package_banner_shows_dismisses_and_stays_off(cx: &mut TestAppContext) {
    let (_dir, root) = node_project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    settle(cx);
    let visible = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| ws.package_banner().visible())
    };
    assert!(visible(&ws, cx));
    let shown = ws.read_with(cx, |ws, _| ws.package_banner().found[0].display());
    assert_eq!(shown, "npm install");

    ws.update(cx, |ws, cx| ws.dismiss_package_banner(cx));
    assert!(!visible(&ws, cx));
    ws.update(cx, |ws, cx| ws.check_packages_now(cx));
    settle(cx);
    assert!(visible(&ws, cx), "the command shows it again");

    // Installing (node_modules appears) clears it on the next check.
    fs::create_dir(root.join("node_modules")).expect("mkdir");
    ws.update(cx, |ws, cx| {
        ws.packages_paths_changed(&[root.join("node_modules")], false, cx)
    });
    settle(cx);
    assert!(!visible(&ws, cx));

    fs::remove_dir(root.join("node_modules")).expect("rmdir");
    ws.update(cx, |ws, cx| ws.never_ask_packages(cx));
    ws.update(cx, |ws, cx| ws.schedule_package_check(cx));
    settle(cx);
    assert!(!visible(&ws, cx));
    let hidden = cx.update(|_, cx| crate::app_state::state(cx).package_banner_hidden(&root));
    assert!(hidden);
}

#[gpui::test]
fn no_banner_for_a_cargo_project_or_when_turned_off(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    settle(cx);
    assert!(ws.read_with(cx, |ws, _| ws.package_banner().found.is_empty()));

    let (_node, node_root) = node_project();
    cx.update(|_, cx| {
        let mut config = Config::default();
        config.workspace.package_banner = false;
        arcode_ui_kit::set_settings(config, cx);
    });
    ws.update_in(cx, |ws, window, cx| ws.open_project(node_root, window, cx));
    settle(cx);
    assert!(ws.read_with(cx, |ws, _| ws.package_banner().found.is_empty()));
}

/// Switching a file to another language drops what the old language's
/// server reported for it: the squiggles and the status bar counts.
#[gpui::test]
fn picking_another_language_drops_the_old_servers_diagnostics(cx: &mut TestAppContext) {
    use arcode_lsp::LspEvent;
    use arcode_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let main_rs = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    open_file(&ws, &root, &main_rs, cx);
    let lsp = ws.read_with(cx, |ws, _| ws.lsp().clone());
    let publish = |cx: &mut VisualTestContext| {
        lsp.update(cx, |lsp, cx| {
            let error = Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 2)),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "mismatched types".into(),
                ..Default::default()
            };
            lsp.handle_event(
                LspEvent::Diagnostics {
                    server: "rust-analyzer".into(),
                    path: main_rs.clone(),
                    version: None,
                    diagnostics: vec![error],
                },
                cx,
            );
        });
        cx.run_until_parked();
    };
    publish(cx);
    let shown = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let editor = ws.active_editor(cx).expect("editor");
            let squiggles = editor.read(cx).diagnostics().len();
            (squiggles, ws.status_bar().read(cx).info().errors)
        })
    };
    assert_eq!(shown(&ws, cx), (1, 1));

    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "plain text", cx);
    assert_eq!(status(&ws, cx).0.as_deref(), Some("Plain Text"));
    assert_eq!(shown(&ws, cx), (0, 0));

    // rust-analyzer still builds main.rs as part of the crate and reports
    // it again: a project-wide problem (counted), but no squiggles in a
    // file that is not Rust any more.
    publish(cx);
    assert_eq!(shown(&ws, cx), (0, 1));
}

/// The picker keeps the selected row in view: it opens scrolled to the
/// current language and follows Up/Down past the visible rows.
#[gpui::test]
fn list_picker_scrolls_to_the_selected_row(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    open_file(&ws, &root, &root.join("src/main.rs"), cx);
    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.list_picker().cloned().expect("picker open"));
    let (selected, offset) = picker.read_with(cx, |p, _| (p.selected(), p.scroll_offset()));
    assert!(selected >= 12, "Rust is past the first rows ({selected})");
    assert_eq!(
        offset,
        crate::list_picker::offset_showing(0., selected),
        "opens scrolled to the current language"
    );
    assert!(offset < 0.);

    // Back to the first row: the list scrolls to the top.
    picker.update(cx, |p, cx| {
        p.set_query("a", cx);
        p.set_query("", cx);
    });
    cx.run_until_parked();
    for _ in 0..3 {
        cx.dispatch_action(crate::finder::SelectNext);
    }
    cx.run_until_parked();
    let (selected, offset) = picker.read_with(cx, |p, _| (p.selected(), p.scroll_offset()));
    assert_eq!(selected, 3);
    assert_eq!(offset, 0.);
}
