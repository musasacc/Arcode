//! Review tests for the git integration (failing tests that reproduce
//! bugs found in review). Each test documents the expected behaviour.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use arcode_buffer::Point;
use gpui::{TestAppContext, VisualTestContext};

use super::tests::open;
use super::*;
use crate::git::editor::{BLAME_SET, ConflictAction};

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new() -> Option<Self> {
        Command::new("git").arg("--version").output().ok()?;
        let dir = tempfile::tempdir().ok()?;
        let root = fs::canonicalize(dir.path()).ok()?.join("repo");
        fs::create_dir(&root).ok()?;
        let repo = Self { _dir: dir, root };
        repo.git(&["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Tester"),
            ("user.email", "t@example.invalid"),
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

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
    }
}

fn open_file<'a>(
    cx: &'a mut TestAppContext,
    repo: &Repo,
    rel: &str,
) -> (Entity<Workspace>, Entity<Editor>, &'a mut VisualTestContext) {
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

/// Two quick clicks on conflict lenses (faster than the 300 ms re-parse
/// debounce) resolve the second conflict with the line numbers it had
/// before the first resolution, deleting the wrong lines. Expected: the
/// second resolution acts on the conflict as it is now (or is refused,
/// like `hunk_action` does for stale hunks), never corrupting the file.
#[gpui::test]
fn review_rapid_conflict_resolutions_use_stale_line_ranges(cx: &mut TestAppContext) {
    let Some(repo) = Repo::new() else { return };
    let text = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feature\n";
    repo.commit("c.txt", text);
    let (ws, editor, cx) = open_file(cx, &repo, "c.txt");
    let git = ws.update(cx, |ws, _| ws.git_editors().clone());
    assert_eq!(
        git.update(cx, |g, _| g.conflicts(editor.entity_id()).len()),
        2
    );

    // Click "Accept Current" on conflict 1, then right away on conflict 2
    // (no time passes in between, as with a quick double action).
    git.update(cx, |g, cx| {
        g.resolve_conflict(&editor, 0, ConflictAction::AcceptCurrent, cx)
    });
    git.update(cx, |g, cx| {
        g.resolve_conflict(&editor, 1, ConflictAction::AcceptCurrent, cx)
    });
    settle(cx);
    let result = editor.update(cx, |e, _| e.buffer().to_string());
    assert!(
        result == "a\nours\nb\nx\n"
            || result == "a\nours\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feature\n",
        "the second resolution used stale line numbers and corrupted the file: {result:?}"
    );
}

/// Inline blame of a dirty buffer passes the LF-normalized buffer text to
/// `git blame --contents -`. For a file committed with CRLF line endings
/// (core.autocrlf=false, the default on Linux/macOS and common in
/// Windows-originated repositories) every line then differs from HEAD, so
/// after typing one character every line is blamed as "Uncommitted".
#[gpui::test]
fn review_inline_blame_of_dirty_crlf_file(cx: &mut TestAppContext) {
    let Some(repo) = Repo::new() else { return };
    repo.commit("w.txt", "alpha\r\nbeta\r\ngamma\r\n");
    let (ws, editor, cx) = open_file(cx, &repo, "w.txt");
    let _ = ws;
    // Edit the last line, then put the cursor on the untouched first line.
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(2, 5)..Point::new(2, 5), "!".into())], cx);
    });
    settle(cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 1)..Point::new(0, 1)], cx)
    });
    settle(cx);
    let texts: Vec<String> = editor.update(cx, |e, _| {
        e.virtual_text(BLAME_SET)
            .iter()
            .map(|t| t.text.to_string())
            .collect()
    });
    assert_eq!(texts.len(), 1, "inline blame on the cursor line: {texts:?}");
    assert!(
        texts[0].starts_with("Tester,"),
        "line 1 is unchanged and committed, but blame says: {:?}",
        texts[0]
    );
}

/// Diff tabs are matched by title only, and the title holds just the file
/// name: opening the diff of `b/mod.rs` closes the open diff of
/// `a/mod.rs`.
#[gpui::test]
fn review_diff_tabs_of_same_named_files_replace_each_other(cx: &mut TestAppContext) {
    let Some(repo) = Repo::new() else { return };
    repo.commit("a/mod.rs", "one\n");
    repo.commit("b/mod.rs", "two\n");
    fs::write(repo.root.join("a/mod.rs"), "ONE\n").unwrap();
    fs::write(repo.root.join("b/mod.rs"), "TWO\n").unwrap();
    let (ws, cx) = open(cx);
    let root = repo.root.clone();
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    settle(cx);
    let (a, b) = (repo.root.join("a/mod.rs"), repo.root.join("b/mod.rs"));
    ws.update_in(cx, |ws, window, cx| ws.open_file_diff(a, false, window, cx));
    settle(cx);
    ws.update_in(cx, |ws, window, cx| ws.open_file_diff(b, false, window, cx));
    settle(cx);
    let tabs = ws.update(cx, |ws, cx| ws.active_pane().read(cx).items().len());
    assert_eq!(tabs, 2, "both diffs should stay open");
}

/// A status refresh that runs while repository discovery is still in
/// progress (a file saved, the Refresh button, `RefreshGit`) finds no
/// active repository and marks the store loaded with no repositories. The
/// Source Control view then shows "This folder is not a git repository"
/// with the Initialize Repository button for a folder that is inside a
/// repository, and clicking it runs `git init` there, creating a nested
/// repository inside the existing one.
#[gpui::test]
fn review_refresh_during_discovery_offers_init_inside_a_repository(cx: &mut TestAppContext) {
    let Some(repo) = Repo::new() else { return };
    repo.commit("sub/a.txt", "a\n");
    let sub = repo.root.join("sub");
    let (ws, cx) = open(cx);
    let store = ws.update(cx, |ws, _| ws.git().clone());
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(sub.clone(), window, cx)
    });
    // Discovery runs in the background; a refresh arrives first.
    store.update(cx, |s, cx| s.refresh(cx));
    let offers_init = store.update(cx, |s, _| s.is_loaded() && s.repos().is_empty());
    if offers_init {
        // What the visible "Initialize Repository" button does.
        store.update(cx, |s, cx| s.init_repository(cx));
    }
    settle(cx);
    assert!(
        !offers_init,
        "the Source Control view offered Initialize Repository inside a repository"
    );
    assert!(
        !sub.join(".git").exists(),
        "a nested repository was created"
    );
}

/// Switching the active repository while a status refresh of the previous
/// one is running publishes that refresh's entries as the status of the
/// new repository (the snapshot does not record which repository it
/// belongs to). For the length of a `git status` the Source Control view
/// lists repository A's files under repository B, and its stage / discard
/// buttons then act on B with A's relative paths.
#[gpui::test]
fn review_status_of_previous_repo_is_shown_for_the_new_active_repo(cx: &mut TestAppContext) {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(dir.path()).unwrap().join("work");
    for name in ["a", "b"] {
        let root = parent.join(name);
        fs::create_dir_all(&root).unwrap();
        let out = Command::new("git")
            .current_dir(&root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    fs::write(parent.join("a/only-in-a.txt"), "x\n").unwrap();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(parent.clone(), window, cx)
    });
    settle(cx);
    let store = ws.update(cx, |ws, _| ws.git().clone());
    assert_eq!(store.update(cx, |s, _| s.repos().len()), 2);
    assert_eq!(store.update(cx, |s, _| s.active_index()), 0);

    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&store, move |store, event: &crate::git::GitEvent, cx| {
            if *event == crate::git::GitEvent::StatusChanged {
                let s = store.read(cx);
                let paths: Vec<String> = s
                    .snapshot()
                    .entries
                    .iter()
                    .map(|e| e.path.display().to_string())
                    .collect();
                log.borrow_mut().push((s.active_index(), paths));
            }
        })
    });
    // A refresh of repository a is running (a file was saved) when the
    // user activates repository b.
    store.update(cx, |s, cx| s.refresh(cx));
    store.update(cx, |s, cx| s.set_active_repo(1, cx));
    settle(cx);
    let seen = seen.borrow();
    for (active, paths) in seen.iter() {
        assert!(
            !(*active == 1 && paths.iter().any(|p| p == "only-in-a.txt")),
            "repository b was shown with repository a's files: {seen:?}"
        );
    }
}
