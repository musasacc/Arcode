//! GPUI tests of the git integration in the workspace: change bars and
//! hunk staging, merge conflicts, init in a folder without git, commit
//! from the Source Control view and diff tabs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{GutterLane, GutterMark};
use gpui::{TestAppContext, VisualTestContext};

use super::tests::open;
use super::*;
use crate::git::editor::{CONFLICT_SET, ConflictAction, GUTTER_SET, HunkAction};

/// A temporary repository with isolated git configuration, or `None` when
/// git is not installed.
struct TestRepo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl TestRepo {
    fn new() -> Option<Self> {
        Command::new("git").arg("--version").output().ok()?;
        let dir = tempfile::tempdir().ok()?;
        let root = fs::canonicalize(dir.path()).ok()?;
        let repo = Self { _dir: dir, root };
        repo.git(&["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            repo.git(&["config", k, v]);
        }
        Some(repo)
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(&self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn commit(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self.git(&["add", "--", rel]);
        self.git(&["commit", "-q", "-m", &format!("add {rel}")]);
    }
}

/// Lets debounce timers fire and background git work finish.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
    }
}

fn open_repo_file(
    cx: &mut TestAppContext,
    repo: &TestRepo,
    rel: &str,
) -> (Entity<Workspace>, Entity<Editor>, &mut VisualTestContext) {
    let (ws, cx) = open(cx);
    let root = repo.root.clone();
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    settle(cx);
    let path = repo.root.join(rel);
    ws.update_in(cx, |ws, window, cx| ws.open_file(&path, window, cx));
    settle(cx);
    let editor = ws.update(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    (ws, editor, cx)
}

fn change_marks(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(usize, GutterMark)> {
    editor.update(cx, |e, _| {
        e.gutter_decorations(GUTTER_SET)
            .iter()
            .filter(|d| d.lane == GutterLane::Changes)
            .map(|d| (d.line, d.mark.clone()))
            .collect()
    })
}

#[gpui::test]
fn change_bars_and_staging_a_hunk(cx: &mut TestAppContext) {
    let Some(repo) = TestRepo::new() else { return };
    repo.commit("src/lib.rs", "one\ntwo\nthree\n");
    let (ws, editor, cx) = open_repo_file(cx, &repo, "src/lib.rs");
    assert!(change_marks(&editor, cx).is_empty(), "clean file");
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.branch_label(cx).as_deref(), Some("main"));
    });

    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(1, 0)..Point::new(1, 3), "TWO".into())], cx);
    });
    settle(cx);
    let marks = change_marks(&editor, cx);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].0, 1);
    assert_eq!(
        marks[0].1,
        GutterMark::Change(arcode_editor::ChangeKind::Modified)
    );

    // Clicking the bar opens the popover; Stage stages the buffer's line.
    let git = ws.update(cx, |ws, _| ws.git_editors().clone());
    git.update(cx, |g, cx| g.toggle_hunk_popover(&editor, 0, cx));
    assert!(editor.update(cx, |e, _| e.has_popover(GUTTER_SET)));
    let id = editor.entity_id();
    git.update(cx, |g, cx| g.hunk_action(id, 0, HunkAction::Stage, cx));
    settle(cx);
    assert!(!editor.update(cx, |e, _| e.has_popover(GUTTER_SET)));
    assert_eq!(
        repo.git(&["cat-file", "blob", ":0:src/lib.rs"]),
        "one\nTWO\nthree\n"
    );
    assert_eq!(
        fs::read_to_string(repo.root.join("src/lib.rs")).unwrap(),
        "one\ntwo\nthree\n"
    );
    let hunks = git.update(cx, |g, _| g.hunks(id).to_vec());
    assert_eq!(hunks.len(), 1);
    assert!(hunks[0].staged, "the staged change still shows, as staged");
    let staged = ws.update(cx, |ws, cx| ws.git().read(cx).snapshot().summary().staged);
    assert_eq!(staged, 1);

    // Unstage, then revert the buffer change.
    git.update(cx, |g, cx| g.hunk_action(id, 0, HunkAction::Unstage, cx));
    settle(cx);
    assert_eq!(
        repo.git(&["cat-file", "blob", ":0:src/lib.rs"]),
        "one\ntwo\nthree\n"
    );
    git.update(cx, |g, cx| g.hunk_action(id, 0, HunkAction::Revert, cx));
    settle(cx);
    assert_eq!(
        editor.update(cx, |e, _| e.buffer().to_string()),
        "one\ntwo\nthree\n"
    );
    assert!(change_marks(&editor, cx).is_empty());
}

#[gpui::test]
fn merge_conflicts_are_highlighted_and_resolved(cx: &mut TestAppContext) {
    let Some(repo) = TestRepo::new() else { return };
    let text = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feature\n";
    repo.commit("c.txt", text);
    let (ws, editor, cx) = open_repo_file(cx, &repo, "c.txt");
    let id = editor.entity_id();
    let git = ws.update(cx, |ws, _| ws.git_editors().clone());
    assert_eq!(git.update(cx, |g, _| g.conflicts(id).len()), 2);
    editor.update(cx, |e, _| {
        assert_eq!(e.code_lenses(CONFLICT_SET).len(), 2);
        assert!(!e.background_highlights(CONFLICT_SET).is_empty());
    });
    let counter = ws.update(cx, |ws, cx| ws.status_bar().read(cx).info().conflicts);
    assert_eq!(counter, Some((None, 2)));

    ws.update(cx, |ws, cx| ws.go_to_conflict(true, cx));
    assert_eq!(editor.update(cx, |e, _| e.cursor_position().line), 1);
    ws.update(cx, |ws, cx| ws.go_to_conflict(true, cx));
    assert_eq!(editor.update(cx, |e, _| e.cursor_position().line), 7);

    git.update(cx, |g, cx| {
        g.resolve_conflict(&editor, 0, ConflictAction::AcceptIncoming, cx)
    });
    settle(cx);
    assert_eq!(git.update(cx, |g, _| g.conflicts(id).len()), 1);
    git.update(cx, |g, cx| {
        g.resolve_conflict(&editor, 0, ConflictAction::AcceptBoth, cx)
    });
    settle(cx);
    assert_eq!(
        editor.update(cx, |e, _| e.buffer().to_string()),
        "a\ntheirs\nb\nx\ny\n"
    );
    assert!(editor.update(cx, |e, _| e.code_lenses(CONFLICT_SET).is_empty()));
    let counter = ws.update(cx, |ws, cx| ws.status_bar().read(cx).info().conflicts);
    assert_eq!(counter, None);
}

#[gpui::test]
fn folder_without_git_offers_init(cx: &mut TestAppContext) {
    let Some(_) = TestRepo::new() else { return };
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    settle(cx);
    let store = ws.update(cx, |ws, _| ws.git().clone());
    store.update(cx, |s, _| {
        assert!(s.is_loaded());
        assert!(s.repos().is_empty());
    });
    assert!(!root.join(".git").exists(), "nothing is created on open");
    store.update(cx, |s, cx| s.init_repository(cx));
    settle(cx);
    assert!(root.join(".git").is_dir());
    store.update(cx, |s, _| {
        assert_eq!(s.repos().len(), 1);
        assert_eq!(s.snapshot().summary().untracked, 1);
    });
}

#[gpui::test]
fn nested_repositories_and_commit(cx: &mut TestAppContext) {
    let Some(repo) = TestRepo::new() else { return };
    repo.commit("README.md", "hi\n");
    // The project is the parent folder holding the repository.
    let parent = repo.root.parent().unwrap().to_path_buf();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(parent.clone(), window, cx)
    });
    settle(cx);
    let store = ws.update(cx, |ws, _| ws.git().clone());
    let found: Vec<PathBuf> = store.update(cx, |s, _| {
        s.repos().iter().map(|r| r.workdir.clone()).collect()
    });
    assert_eq!(found, [repo.root.clone()]);

    fs::write(repo.root.join("README.md"), "hello\n").unwrap();
    let panel = ws.update(cx, |ws, _| ws.git_panel().clone());
    panel.update(cx, |p, cx| p.stage_file(Path::new("README.md"), cx));
    settle(cx);
    panel.update(cx, |p, cx| {
        p.message().update(cx, |m, cx| m.set_text("Say hello", cx))
    });
    panel.update_in(cx, |p, window, cx| {
        p.commit(&crate::actions::GitCommit, window, cx)
    });
    settle(cx);
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "Say hello");
    assert!(panel.update(cx, |p, cx| p.message().read(cx).text().is_empty()));
    assert!(store.update(cx, |s, _| s.snapshot().entries.is_empty()));
}

#[gpui::test]
fn diff_tab_shows_changes_read_only(cx: &mut TestAppContext) {
    let Some(repo) = TestRepo::new() else { return };
    repo.commit("a.txt", "one\ntwo\n");
    fs::write(repo.root.join("a.txt"), "one\n2\n").unwrap();
    let (ws, cx) = open(cx);
    let root = repo.root.clone();
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    settle(cx);
    let path = repo.root.join("a.txt");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file_diff(path.clone(), false, window, cx)
    });
    settle(cx);
    let editor = ws.update(cx, |ws, cx| ws.active_editor(cx).unwrap());
    editor.update(cx, |e, _| {
        assert!(e.is_read_only());
        assert!(
            e.path().is_none(),
            "a diff tab can never be saved over a file"
        );
        assert_eq!(e.buffer().to_string(), "one\ntwo\n2\n");
    });
    let title = ws.update(cx, |ws, cx| ws.active_pane().read(cx).title_of(&editor, cx));
    assert_eq!(title.as_ref(), "a.txt (HEAD ↔ Working Tree)");
    // Opening it again replaces the tab instead of adding one.
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file_diff(path, false, window, cx)
    });
    settle(cx);
    assert_eq!(ws.update(cx, |ws, cx| ws.editors(cx).len()), 1);
}
