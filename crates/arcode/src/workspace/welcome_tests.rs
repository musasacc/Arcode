//! The welcome screen: pinning, removing, clearing, the filter and its
//! keys, missing folders, and cloning a (local) repository.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::open;
use crate::actions::{ClearRecentProjects, CloneRepository};
use crate::app_state;
use crate::welcome::Welcome;

#[path = "welcome_list_tests.rs"]
mod list_tests;

/// `names` as project folders in a temporary folder, opened oldest first
/// (so the last one is the most recent).
pub(super) fn seed(
    names: &[&str],
    cx: &mut VisualTestContext,
) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let paths: Vec<PathBuf> = names.iter().map(|n| root.join(n)).collect();
    for path in &paths {
        std::fs::create_dir_all(path).unwrap();
    }
    cx.update(|_, cx| {
        for path in &paths {
            app_state::touch_recent(path, cx);
        }
    });
    (dir, paths)
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

pub(super) fn welcome(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<Welcome> {
    ws.read_with(cx, |ws, _| ws.welcome().clone())
}

pub(super) fn names(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    let welcome = welcome(ws, cx);
    welcome.read_with(cx, |w, cx| w.visible_names(cx))
}

pub(super) fn toast(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, _| {
        ws.toast.as_ref().map(|t| t.to_string()).unwrap_or_default()
    })
}

#[gpui::test]
fn pin_remove_and_clear(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (_dir, paths) = seed(&["alpha", "beta", "gamma"], cx);
    draw(cx);
    assert_eq!(names(&ws, cx), ["gamma", "beta", "alpha"]);

    // Pinning the oldest puts it first, under "Pinned".
    let w = welcome(&ws, cx);
    w.update(cx, |w, cx| w.toggle_pin(&paths[0], cx));
    draw(cx);
    assert_eq!(names(&ws, cx), ["alpha", "gamma", "beta"]);
    cx.update(|_, cx| assert!(app_state::recent_projects(cx).iter().any(|p| p.pinned)));

    // Removing forgets the project, not the folder.
    w.update(cx, |w, cx| w.remove(&paths[1], cx));
    draw(cx);
    assert_eq!(names(&ws, cx), ["alpha", "gamma"]);
    assert!(paths[1].is_dir());

    // Clearing keeps the pinned project.
    cx.dispatch_action(ClearRecentProjects);
    draw(cx);
    assert_eq!(names(&ws, cx), ["alpha"]);
    assert!(toast(&ws, cx).contains("Cleared 1"), "{}", toast(&ws, cx));

    // Unpinning makes it an ordinary entry again.
    w.update(cx, |w, cx| w.toggle_pin(&paths[0], cx));
    cx.update(|_, cx| assert!(!app_state::recent_projects(cx)[0].pinned));
}

#[gpui::test]
fn typing_filters_and_the_keys_pick_a_project(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let names_in: Vec<String> = (0..11).map(|i| format!("project-{i:02}")).collect();
    let mut all: Vec<&str> = names_in.iter().map(String::as_str).collect();
    all.push("zebra-tool");
    let (_dir, paths) = seed(&all, cx);
    draw(cx);
    let w = welcome(&ws, cx);
    assert!(w.read_with(cx, |w, cx| w.shows_filter(cx)));
    assert_eq!(names(&ws, cx).len(), 12);

    // Typing anywhere on the screen goes into the filter.
    cx.simulate_keystrokes("z e b");
    draw(cx);
    assert_eq!(w.read_with(cx, |w, cx| w.filter_text(cx).to_owned()), "zeb");
    assert_eq!(names(&ws, cx), ["zebra-tool"]);

    // Escape clears the filter.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(names(&ws, cx).len(), 12);

    // Down / Up move the selection; Enter opens it.
    cx.simulate_keystrokes("down down up down");
    let selected = w.read_with(cx, |w, cx| w.selected_path(cx));
    assert_eq!(selected.as_deref(), Some(paths[10].as_path()), "second row");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    assert_eq!(root.as_deref(), Some(paths[10].as_path()));
}

#[gpui::test]
fn enter_opens_the_first_match(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let many: Vec<String> = (0..10).map(|i| format!("p{i}")).collect();
    let mut all: Vec<&str> = many.iter().map(String::as_str).collect();
    all.push("needle");
    let (_dir, paths) = seed(&all, cx);
    draw(cx);
    cx.simulate_keystrokes("n e e d enter");
    cx.run_until_parked();
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    assert_eq!(root.as_deref(), paths.last().map(PathBuf::as_path));
}

#[gpui::test]
fn few_projects_have_no_filter(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (_dir, _paths) = seed(&["one", "two"], cx);
    draw(cx);
    let w = welcome(&ws, cx);
    assert!(!w.read_with(cx, |w, cx| w.shows_filter(cx)));
    cx.simulate_keystrokes("x");
    assert_eq!(w.read_with(cx, |w, cx| w.filter_text(cx).to_owned()), "");
}

#[gpui::test]
fn missing_folders_are_marked_and_explained(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (_dir, paths) = seed(&["here", "gone"], cx);
    std::fs::remove_dir(&paths[1]).unwrap();
    draw(cx);
    let w = welcome(&ws, cx);
    assert!(w.read_with(cx, |w, _| w.is_missing(&paths[1])));
    assert!(!w.read_with(cx, |w, _| w.is_missing(&paths[0])));
    // Still listed, dimmed, until removed.
    assert_eq!(names(&ws, cx), ["gone", "here"]);

    w.update_in(cx, |w, window, cx| {
        w.open(paths[1].clone(), false, window, cx)
    });
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.project().is_none()));
    assert!(
        toast(&ws, cx).contains("no longer exists"),
        "{}",
        toast(&ws, cx)
    );

    w.update(cx, |w, cx| w.remove(&paths[1], cx));
    draw(cx);
    assert_eq!(names(&ws, cx), ["here"]);
}

/// `path` without the `\\?\` prefix `canonicalize` adds on Windows,
/// which nobody types into the dialog.
fn plain_path(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

/// A git repository with one commit, or `None` without git.
pub(super) fn source_repo() -> Option<(tempfile::TempDir, PathBuf)> {
    let git = |dir: &Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let dir = tempfile::tempdir().unwrap();
    let repo = plain_path(std::fs::canonicalize(dir.path()).unwrap()).join("demo-repo");
    std::fs::create_dir(&repo).unwrap();
    if !git(&repo, &["init", "-q"]) {
        return None;
    }
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    let ok = git(&repo, &["add", "."]) && git(&repo, &["commit", "-q", "-m", "first"]);
    ok.then_some((dir, repo))
}

#[gpui::test]
fn cloning_a_local_repository_opens_it(cx: &mut TestAppContext) {
    let Some((_src, repo)) = source_repo() else {
        return;
    };
    let dest = tempfile::tempdir().unwrap();
    let dest_root = plain_path(std::fs::canonicalize(dest.path()).unwrap());
    let (ws, cx) = open(cx);
    cx.dispatch_action(CloneRepository);
    cx.run_until_parked();
    let dialog = ws
        .read_with(cx, |ws, _| ws.clone_dialog().cloned())
        .expect("the dialog is open");
    dialog.update(cx, |d, cx| {
        d.set_location(&dest_root.display().to_string(), cx);
        d.set_url(&repo.display().to_string(), cx);
    });
    cx.run_until_parked();
    // The folder name follows the address until it is edited.
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "demo-repo");
    dialog.update(cx, |d, cx| d.set_folder("my-clone", cx));
    dialog.update(cx, |d, cx| d.set_url(&format!("{}/", repo.display()), cx));
    cx.run_until_parked();
    assert_eq!(dialog.read_with(cx, |d, cx| d.folder(cx)), "my-clone");

    cx.simulate_keystrokes("enter");
    let target = dest_root.join("my-clone");
    for _ in 0..1200 {
        cx.run_until_parked();
        if ws.read_with(cx, |ws, _| ws.project().is_some()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    let state = dialog.read_with(cx, |d, _| (d.is_running(), d.error().map(str::to_owned)));
    // The project root is canonical (`\\?\C:\...` on Windows).
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    assert_eq!(
        root.as_deref().map(canonical),
        Some(canonical(&target)),
        "(running, error) = {state:?}"
    );
    assert!(target.join("README.md").is_file());
    assert!(ws.read_with(cx, |ws, _| ws.clone_dialog().is_none()));
    let recent = cx.update(|_, cx| app_state::recent_projects(cx)[0].path.clone());
    assert_eq!(canonical(&recent), canonical(&target));
}

#[gpui::test]
fn a_failed_clone_keeps_the_dialog_with_the_reason(cx: &mut TestAppContext) {
    if source_repo().is_none() {
        return;
    }
    let dest = tempfile::tempdir().unwrap();
    let (ws, cx) = open(cx);
    cx.dispatch_action(CloneRepository);
    cx.run_until_parked();
    let dialog = ws
        .read_with(cx, |ws, _| ws.clone_dialog().cloned())
        .expect("the dialog is open");
    // Somewhere other than the destination, so the clone target (named
    // after the address) is not the address itself.
    let elsewhere = tempfile::tempdir().unwrap();
    let missing = elsewhere.path().join("no-such-repo");
    dialog.update(cx, |d, cx| {
        d.set_location(&dest.path().display().to_string(), cx);
        d.set_url(&missing.display().to_string(), cx);
    });
    cx.simulate_keystrokes("enter");
    let mut error = None;
    for _ in 0..1200 {
        cx.run_until_parked();
        error = dialog.read_with(cx, |d, _| d.error().map(str::to_owned));
        if error.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let running = dialog.read_with(cx, |d, _| d.is_running());
    let opened = ws.read_with(cx, |ws, _| ws.project_root().map(Path::to_path_buf));
    let error = error.unwrap_or_else(|| {
        panic!("no error shown after 60 s; running: {running}, opened: {opened:?}")
    });
    assert!(error.contains("not found"), "{error}");
    assert!(!dialog.read_with(cx, |d, _| d.is_running()));
    assert!(!dest.path().join("no-such-repo").exists());
    // Escape closes the dialog when nothing runs.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.clone_dialog().is_none()));
}
