//! Reviewer tests for the missing-packages check (`missing_packages_with`).
//! Each one pins a defect found in review (now fixed).

use std::fs;
use std::path::{Path, PathBuf};

use arcode_project::missing_packages_with;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    (dir, root)
}

fn cache_has(cache: &Path, folder: &str) {
    fs::create_dir_all(cache.join(folder)).unwrap();
}

/// `replace old => new vX` points `old` at another module: `go mod download`
/// fetches `new@vX` into the module cache, never `old@<required version>`.
/// The check still looks for `old@<required version>`, so after a complete
/// `go mod download` the banner keeps saying packages are missing (forever,
/// for any project that replaces a dependency with a fork or pins another
/// version). Expected: a replaced requirement is looked up under its
/// replacement.
#[test]
fn review_go_remote_replace_is_checked_under_its_replacement() {
    let (dir, root) = project();
    let cache = dir.path().join("gomodcache");
    fs::write(
        root.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\n\
         require github.com/upstream/lib v1.0.0\n\n\
         replace github.com/upstream/lib => github.com/fork/lib v1.0.1\n",
    )
    .unwrap();
    // What `go mod download` leaves in the cache for this go.mod.
    cache_has(&cache, "github.com/fork/lib@v1.0.1");
    let found = missing_packages_with(&root, &[], Some(&cache));
    assert!(
        found.is_empty(),
        "every module go needs is in the cache, yet reported: {:?}",
        found.iter().map(|m| m.display()).collect::<Vec<_>>()
    );
}

/// A `replace` whose target is a Windows absolute path (`C:\src\lib`,
/// `D:/work/lib`) is a local directory, never downloaded. Only targets that
/// start with `.` or `/` count as local, so on Windows such a project shows
/// the banner forever (the module can never appear in the cache).
#[test]
fn review_go_windows_absolute_replace_counts_as_local() {
    let (dir, root) = project();
    let cache = dir.path().join("gomodcache");
    fs::write(
        root.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\n\
         require example.com/lib v0.0.0\n\n\
         replace example.com/lib => C:\\src\\lib\n",
    )
    .unwrap();
    fs::create_dir_all(&cache).unwrap();
    let found = missing_packages_with(&root, &[], Some(&cache));
    assert!(
        found.is_empty(),
        "a local replace was reported as missing: {:?}",
        found.iter().map(|m| m.display()).collect::<Vec<_>>()
    );
}
