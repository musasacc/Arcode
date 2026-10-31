//! Reviewer tests for the package banner, language packs and status bar
//! unit. Each test pins a defect found in review (now fixed).

use std::fs;
use std::path::{Path, PathBuf};

use arcode_config::Config;
use arcode_editor::Editor;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::open;
use crate::languages::actions::SelectLanguageMode;
use crate::sidebar::file_tree::FileTreeEvent;
use crate::test_support::settle;

/// `debugger::editors::RUN_SET` (that module is private to the debugger).
const RUN_SET: &str = "debugger-run";

const MAIN: &str = "fn main() {\n    println!(\"hi\");\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {}\n}\n";

fn cargo_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join(".git")).expect("git");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/main.rs"), MAIN).expect("write");
    (dir, root)
}

fn open_file(
    ws: &Entity<Workspace>,
    root: &Path,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<Editor> {
    let (root, path) = (root.to_path_buf(), path.to_path_buf());
    ws.update_in(cx, |ws, window, cx| {
        if ws.project().is_none() {
            ws.open_project(root, window, cx);
        }
        ws.open_file(&path, window, cx);
    });
    settle(cx);
    ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("editor"))
}

fn pick(ws: &Entity<Workspace>, query: &str, cx: &mut VisualTestContext) {
    let picker = ws.read_with(cx, |ws, _| ws.list_picker().cloned().expect("picker open"));
    picker.update(cx, |p, cx| {
        p.set_query(query, cx);
        p.confirm(cx);
    });
    settle(cx);
}

fn run_lines(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<usize> {
    editor.read_with(cx, |e, _| {
        e.gutter_decorations(RUN_SET)
            .iter()
            .map(|d| d.line)
            .collect()
    })
}

fn language(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> &'static str {
    editor.read_with(cx, |e, _| e.language().id)
}

/// Turning a language pack off on the Languages page is documented (and the
/// switch's tooltip says) "no language servers or run buttons". The debugger
/// skips the recomputation for a disabled language but keeps the run
/// buttons it already found, so an open file keeps its ▶ buttons (and they
/// still run `cargo run`). Seen in the app: rp/src/main.rs keeps three run
/// buttons after the Rust switch is turned off.
#[gpui::test]
fn review_disabling_a_language_removes_run_buttons_of_open_files(cx: &mut TestAppContext) {
    let (_dir, root) = cargo_project();
    let (ws, cx) = open(cx);
    let editor = open_file(&ws, &root, &root.join("src/main.rs"), cx);
    assert!(
        !run_lines(&editor, cx).is_empty(),
        "precondition: run buttons shown"
    );

    let (config, warnings) = Config::from_toml_str("[language.rust]\nenabled = false\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    cx.update(|_, cx| arcode_ui_kit::set_settings(config, cx));
    settle(cx);
    assert_eq!(
        run_lines(&editor, cx),
        Vec::<usize>::new(),
        "run buttons of a disabled language are still shown"
    );
}

/// Picking "Plain Text" (or any other language) in the language picker
/// keeps the run buttons of the old language: the debugger is never told
/// that the language changed. Seen in the app: main.rs shown as Plain Text
/// with ▶ on `fn main`, `mod tests` and the test.
#[gpui::test]
fn review_picking_another_language_removes_stale_run_buttons(cx: &mut TestAppContext) {
    let (_dir, root) = cargo_project();
    let (ws, cx) = open(cx);
    let editor = open_file(&ws, &root, &root.join("src/main.rs"), cx);
    assert!(
        !run_lines(&editor, cx).is_empty(),
        "precondition: run buttons shown"
    );

    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "plain text", cx);
    assert_eq!(language(&editor, cx), "plaintext");
    assert_eq!(
        run_lines(&editor, cx),
        Vec::<usize>::new(),
        "a Plain Text file still shows Rust run buttons"
    );
}

/// "A language picked by hand sticks for that editor until Auto Detect is
/// picked." Renaming the file breaks that: `Editor::set_path` resets the
/// language from the new name, and `reapply_association` returns early for
/// hand-picked editors, so nothing puts the picked language back (the
/// editor stays in `langs.manual`, so associations no longer apply to it
/// either).
#[gpui::test]
fn review_rename_keeps_a_language_picked_by_hand(cx: &mut TestAppContext) {
    let (_dir, root) = cargo_project();
    let notes = root.join("notes");
    fs::write(&notes, "# Title\n\nSome *text*.\n").expect("write");
    let (ws, cx) = open(cx);
    let editor = open_file(&ws, &root, &notes, cx);
    cx.dispatch_action(SelectLanguageMode);
    cx.run_until_parked();
    pick(&ws, "markdown", cx);
    assert_eq!(language(&editor, cx), "markdown");

    let renamed = root.join("notes-old");
    fs::rename(&notes, &renamed).expect("rename");
    ws.update_in(cx, |ws, window, cx| {
        ws.on_tree_event(
            &FileTreeEvent::Renamed {
                from: notes.clone(),
                to: renamed.clone(),
            },
            window,
            cx,
        )
    });
    settle(cx);
    assert_eq!(
        language(&editor, cx),
        "markdown",
        "the hand-picked language was lost by a rename"
    );
}

/// `composer install` writes `vendor/` (gitignored in practically every PHP
/// project, so the watcher never reports it) and `composer.lock`. The
/// banner re-checks only for the names in `packages::is_relevant`, which
/// lists `composer.json` but not `composer.lock`, so after a successful
/// install run outside the banner (or after Dismiss) it stays up for the
/// whole session. (The same holds for `Pipfile.lock`, `pdm.lock` and
/// `.pnp.js`.)
#[gpui::test]
fn review_composer_lock_change_rechecks_the_banner(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join(".git")).expect("git");
    fs::write(root.join(".gitignore"), "vendor/\n").expect("write");
    fs::write(
        root.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    settle(cx);
    assert!(
        ws.read_with(cx, |ws, _| ws.package_banner().visible()),
        "precondition: banner shown"
    );

    // `composer install` ran in a terminal.
    fs::create_dir(root.join("vendor")).expect("mkdir");
    fs::write(root.join("composer.lock"), "{}").expect("write");
    // What the git watcher reports: vendor/ is ignored, composer.lock is not.
    ws.update(cx, |ws, cx| {
        ws.packages_paths_changed(&[root.join("composer.lock")], false, cx)
    });
    settle(cx);
    assert!(
        !ws.read_with(cx, |ws, _| ws.package_banner().visible()),
        "the banner still says packages are missing after composer install"
    );
}
