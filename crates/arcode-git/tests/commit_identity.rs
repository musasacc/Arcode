//! `commit` without a configured identity. Kept in its own test binary
//! because it changes this process's environment.

mod common;

use arcode_git::{GitError, commit, stage_file};
use common::{TestRepo, p};

#[test]
fn missing_identity_is_reported() {
    let Some(t) = TestRepo::new() else { return };
    let empty = t.dir.parent().unwrap().join("empty-global");
    std::fs::write(&empty, "").unwrap();
    // SAFETY: this binary has a single test, so no other thread reads the
    // environment concurrently.
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        for var in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "EMAIL",
        ] {
            std::env::remove_var(var);
        }
    }
    t.git(&["config", "--unset", "user.name"]);
    t.git(&["config", "--unset", "user.email"]);
    // Never guess an identity from the host name.
    t.git(&["config", "user.useConfigOnly", "true"]);

    let repo = t.repo();
    t.write("a.txt", "a\n");
    stage_file(&repo, p("a.txt")).unwrap();
    let err = commit(&repo, "first").unwrap_err();
    assert!(matches!(err, GitError::IdentityNotConfigured(_)), "{err:?}");
    assert!(err.to_string().contains("user.name"));
}
