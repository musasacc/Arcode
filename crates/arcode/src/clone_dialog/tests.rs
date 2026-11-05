use std::path::PathBuf;

use arcode_git::CloneFailure;
use arcode_project::Shell;

use super::*;

#[test]
fn folder_follows_the_address() {
    assert_eq!(proposed_folder(""), "");
    assert_eq!(proposed_folder("  "), "");
    assert_eq!(proposed_folder("https://github.com/o/arcode.git"), "arcode");
    assert_eq!(proposed_folder("git@github.com:o/arcode.git/"), "arcode");
}

#[test]
fn requests_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().display().to_string();
    let ok = check_request(" https://h/o/r.git ", &parent, " r ").unwrap();
    assert_eq!(ok.url, "https://h/o/r.git");
    assert_eq!(ok.folder, "r");
    assert_eq!(ok.target(), dir.path().join("r"));

    let err =
        |url: &str, location: &str, folder: &str| check_request(url, location, folder).unwrap_err();
    assert!(err("", &parent, "r").contains("address"));
    assert!(err("--upload-pack=x", &parent, "r").contains("not a repository"));
    assert!(err("https://h/a b", &parent, "r").contains("not a repository"));
    assert!(err("git@h:o/a b.git", &parent, "r").contains("not a repository"));
    assert!(err("https://h/r", &parent, "").contains("name"));
    assert!(err("https://h/r", &parent, "a/b").contains("cannot contain"));
    assert!(err("https://h/r", &parent, "..").contains("cannot contain"));
    assert!(err("https://h/r", "", "r").contains("where"));
    assert!(err("https://h/r", "relative/dir", "r").contains("full path"));
    let itself = dir.path().join("r").display().to_string();
    assert!(err(&itself, &parent, "r").contains("the same"));

    // An empty folder is fine (git clones into it); one with files is not.
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    assert!(check_request("https://h/r", &parent, "empty").is_ok());
    std::fs::create_dir(dir.path().join("full")).unwrap();
    std::fs::write(dir.path().join("full/a.txt"), "a").unwrap();
    let not_empty = err("https://h/r", &parent, "full");
    assert!(not_empty.contains("not empty"), "{not_empty}");
    // Written the way the dialog writes paths (with ~ for home).
    let home = std::env::home_dir();
    let shown = crate::welcome::tilde_path(&dir.path().join("full"), home.as_deref());
    assert!(not_empty.starts_with(&shown), "{not_empty}");
    std::fs::write(dir.path().join("file"), "a").unwrap();
    assert!(err("https://h/r", &parent, "file").contains("already exists"));
}

#[test]
fn local_paths_may_contain_spaces() {
    for path in [
        "/srv/My Repos/demo",
        "~/My Repos/demo",
        "./my repo",
        r"C:\Users\Jane Doe\repo",
        "c:/Users/Jane Doe/repo",
        r"\\server\share\my repo",
        "file:///srv/My Repos/demo",
    ] {
        assert!(is_local_path(path), "{path}");
    }
    for url in ["https://h/o/r", "git@h:o/r.git", "ssh://h/r", "h:r"] {
        assert!(!is_local_path(url), "{url}");
    }

    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().display().to_string();
    let ok = check_request("/srv/My Repos/demo", &parent, "demo").unwrap();
    assert_eq!(ok.url, "/srv/My Repos/demo");
    // git runs without a shell, so ~ is expanded here.
    let ok = check_request("~/My Repos/demo", &parent, "demo").unwrap();
    let expected = crate::path_prompt::expand_home("~/My Repos/demo");
    assert_eq!(ok.url, expected.display().to_string());
}

#[test]
fn terminal_command_is_quoted_for_the_shell() {
    let request = CloneRequest {
        url: "git@host:o/r.git".into(),
        parent: PathBuf::from("/home/me/my code"),
        folder: "r".into(),
    };
    assert_eq!(
        terminal_command(Shell::Posix, &request, true),
        "git clone git@host:o/r.git r"
    );
    // The target is joined with the platform's separator.
    let target = request.target().display().to_string();
    assert!(target.starts_with("/home/me/my code") && target.ends_with('r'));
    assert_eq!(
        terminal_command(Shell::Posix, &request, false),
        format!("git clone git@host:o/r.git '{target}'")
    );
    let odd = CloneRequest {
        url: "https://h/it's.git".into(),
        ..request
    };
    assert_eq!(
        terminal_command(Shell::Posix, &odd, true),
        r"git clone 'https://h/it'\''s.git' r"
    );
}

#[test]
fn failures_explain_what_to_do() {
    let auth = JobError::Failed(CloneFailure::AuthRequired);
    assert!(failure_message(&auth).contains("credential helper"));
    assert!(failure_message(&auth).contains("Clone in Terminal"));
    assert!(needs_terminal(&auth));
    assert!(needs_terminal(&JobError::Failed(
        CloneFailure::UnknownHostKey
    )));
    assert!(failure_message(&JobError::GitMissing).contains("not installed"));
    assert!(failure_message(&JobError::Failed(CloneFailure::Network)).contains("proxy"));
    // A cancelled clone is what the user asked for, not an error.
    assert!(!is_problem(&JobError::Cancelled));
    assert!(is_problem(&auth));
    assert!(!needs_terminal(&JobError::GitMissing));
    assert_eq!(
        failure_message(&JobError::Failed(CloneFailure::Other("early EOF".into()))),
        "git clone failed: early EOF"
    );
}

#[test]
fn a_failed_clone_never_touches_an_occupied_folder() {
    let dir = tempfile::tempdir().unwrap();
    let occupied = dir.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("mine.txt"), "keep").unwrap();
    job::clean_up(&occupied, job::Before::of(&occupied));
    assert!(occupied.join("mine.txt").exists());

    let fresh = dir.path().join("fresh");
    let before = job::Before::of(&fresh);
    std::fs::create_dir_all(fresh.join(".git")).unwrap();
    job::clean_up(&fresh, before);
    assert!(!fresh.exists());

    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let before = job::Before::of(&empty);
    std::fs::create_dir_all(empty.join(".git/objects")).unwrap();
    job::clean_up(&empty, before);
    assert!(empty.is_dir());
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
}

#[test]
fn cloning_a_missing_local_repository_fails_as_not_found() {
    let git_works = std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !git_works {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-repo");
    let target = dir.path().join("clone");
    let (_job, events) = job::CloneJob::start(missing.display().to_string(), target.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut seen = Vec::new();
    let finished = loop {
        match events.try_recv() {
            Ok(job::JobEvent::Finished(result)) => break Some(result),
            Ok(event) => seen.push(event),
            Err(async_channel::TryRecvError::Closed) => break None,
            Err(async_channel::TryRecvError::Empty) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the clone did not finish in 60 s; events: {seen:?}"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    };
    assert_eq!(
        finished,
        Some(Err(job::JobError::Failed(CloneFailure::NotFound))),
        "events: {seen:?}"
    );
    assert!(!target.exists());
}
