//! Reviewer tests for the file-tree operations (`arcode_project::files`).

use arcode_project::files::rename;

/// On a case-sensitive file system (the Linux default) `a.txt` and `A.txt`
/// are two different files. Renaming `a.txt` to `A.txt` is treated as a
/// "case-only" rename, so the "already exists" check is skipped and
/// `std::fs::rename` silently replaces the other file: its content is lost
/// and nothing goes to the trash.
#[test]
fn review_case_only_rename_must_not_overwrite_a_distinct_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lower = dir.path().join("a.txt");
    let upper = dir.path().join("A.txt");
    std::fs::write(&lower, "lower").expect("write");
    std::fs::write(&upper, "UPPER").expect("write");
    // Only meaningful on a case-sensitive file system.
    if std::fs::read_to_string(&lower).expect("read") != "lower"
        || std::fs::read_dir(dir.path()).expect("ls").count() != 2
    {
        return;
    }
    let result = rename(&lower, "A.txt");
    let upper_now = std::fs::read_to_string(&upper).unwrap_or_default();
    assert!(
        result.is_err() && upper_now == "UPPER",
        "rename returned {result:?}; A.txt now holds {upper_now:?} (the original \
         \"UPPER\" content was overwritten)"
    );
}

/// A file whose name is not valid UTF-8 (possible on Linux) must keep its
/// exact name in the trash, and its `.trashinfo` must record the original
/// path, so "Restore" puts it back where it was. `trash_into` uses
/// `to_string_lossy`, so the name becomes `bad\u{FFFD}.txt` and the stored
/// `Path=` points at a file that never existed.
#[cfg(target_os = "linux")]
#[test]
fn review_trash_keeps_non_utf8_names() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use arcode_project::files::trash_into;

    let dir = tempfile::tempdir().expect("tempdir");
    let name = OsStr::from_bytes(b"bad\xff.txt");
    let file = dir.path().join(name);
    std::fs::write(&file, "x").expect("write");
    let trash = dir.path().join("Trash");
    let to = trash_into(&trash, &file).expect("trashed");
    assert_eq!(
        to.file_name(),
        Some(name),
        "the trashed file was renamed to {to:?}"
    );
    let info = std::fs::read_dir(trash.join("info"))
        .expect("info dir")
        .flatten()
        .next()
        .expect("one .trashinfo");
    let body = std::fs::read_to_string(info.path()).expect("read info");
    assert!(
        body.contains("bad%FF.txt"),
        "the original path is not recorded byte-exactly: {body}"
    );
}
