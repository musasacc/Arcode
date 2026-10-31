//! Review tests of the debugger: each one reproduces a bug found in review
//! (they fail until the bug is fixed).

use std::path::{Path, PathBuf};

use arcode_buffer::{Buffer, Point};
use arcode_editor::Editor;
use gpui::{AppContext, Entity, TestAppContext};

use super::Debugger;
#[cfg(unix)]
use super::launch::{LaunchChoice, LaunchKind};
use crate::test_support::{init, settle};

const MAIN: &str = "fn add(a: i32) -> i32 {\n    a + 1\n}\n\nfn main() {\n    let x = add(1);\n    println!(\"{x}\");\n}\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/main.rs"), MAIN).expect("write");
    (dir, root)
}

fn setup(cx: &mut TestAppContext, root: &Path) -> (Entity<Debugger>, Entity<Editor>) {
    init(cx);
    let debugger = cx.new(Debugger::new);
    debugger.update(cx, |d, cx| d.set_project(Some(root.to_path_buf()), &[], cx));
    let path = root.join("src/main.rs");
    let editor = cx.new(|cx| Editor::new(Buffer::from_file(&path).expect("open"), cx));
    debugger.update(cx, |d, cx| d.register_editor(&editor, cx));
    settle(cx);
    (debugger, editor)
}

fn lines(cx: &mut TestAppContext, debugger: &Entity<Debugger>) -> Vec<usize> {
    debugger.read_with(cx, |d, _| d.book().all().iter().map(|b| b.line).collect())
}

/// While paused on a line that has a breakpoint, the stopped arrow replaces
/// the dot (so the dot is not in the editor's BREAKPOINT_SET). Any edit then
/// runs `follow_edits`, which treats every breakpoint that is not drawn as
/// "deleted with its line" and removes it from the book (and from state.toml).
#[gpui::test]
fn a_breakpoint_under_the_stopped_arrow_survives_an_unrelated_edit(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, editor) = setup(cx, &root);
    let path = root.join("src/main.rs");
    debugger.update(cx, |d, cx| {
        d.toggle_breakpoint(&path, 1, cx);
        d.toggle_breakpoint(&path, 5, cx);
    });
    cx.run_until_parked();
    assert_eq!(lines(cx, &debugger), [1, 5]);

    // Paused at the breakpoint on line 5 (what `select_frame` sets after a
    // stop).
    debugger.update(cx, |d, cx| {
        d.stopped_at = Some((path.clone(), 5));
        d.refresh_all_editors(cx);
    });
    cx.run_until_parked();

    // Type one space on line 1: no line moves.
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(1, 0)..Point::new(1, 0), " ".into())], cx)
    });
    cx.run_until_parked();
    assert_eq!(
        lines(cx, &debugger),
        [1, 5],
        "the breakpoint on the stopped line was deleted by an edit on another line"
    );
}

/// `start` snapshots the breakpoints into `PrepareInput` before the build
/// runs (a cargo build can take minutes). Breakpoints added or removed while
/// it builds are not sent: `send_breakpoints` does nothing without a
/// session, and `launch` starts the adapter with the stale snapshot.
#[cfg(unix)]
#[gpui::test]
fn breakpoints_changed_while_building_reach_the_adapter(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, _editor) = setup(cx, &root);
    let path = root.join("src/main.rs");
    debugger.update(cx, |d, cx| d.toggle_breakpoint(&path, 1, cx));
    cx.run_until_parked();

    // A launch.toml entry with a stand-in adapter process (never answers;
    // only the breakpoints the session was configured with matter here).
    let (entries, warnings) = arcode_dap::parse_launch_file(
        "[[configuration]]\nname = \"fake\"\nprogram = \"/bin/true\"\n\
         [configuration.adapter_command]\ncommand = \"sleep\"\nargs = [\"3\"]\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let choice = LaunchChoice {
        name: "fake".into(),
        kind: LaunchKind::File(Box::new(entries[0].clone())),
    };
    debugger.update(cx, |d, cx| d.start(choice, None, cx));
    assert!(debugger.read_with(cx, |d, _| d.preparing().is_some()));

    // The user adds a breakpoint while the program is being built.
    debugger.update(cx, |d, cx| d.toggle_breakpoint(&path, 5, cx));
    settle(cx);

    let sent: Vec<u32> = debugger.read_with(cx, |d, _| {
        let session = d.session().expect("the session started");
        session
            .handle
            .breakpoints()
            .get(&path)
            .map(|list| list.iter().map(|b| b.line).collect())
            .unwrap_or_default()
    });
    debugger.update(cx, |d, cx| d.stop_session(true, cx));
    assert_eq!(
        sent,
        [2, 6],
        "the breakpoint added during the build was never sent to the adapter"
    );
}
