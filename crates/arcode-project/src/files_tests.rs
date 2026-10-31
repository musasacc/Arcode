use std::path::{Path, PathBuf};

use super::*;

#[test]
fn names_are_validated() {
    assert!(validate_name("main.rs").is_ok());
    assert!(validate_name(".env").is_ok());
    for bad in [
        "", "  ", ".", "..", "a/b", "a\\b", "a:b", " lead", "trail ", "a\nb",
    ] {
        assert!(validate_name(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn create_rename_move_delete() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = create_file(root, "a.txt").unwrap();
    assert!(file.is_file());
    assert_eq!(
        create_file(root, "a.txt").unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let sub = create_dir(root, "sub").unwrap();
    assert!(sub.is_dir());
    assert!(create_dir(root, "x/y").is_err());

    // Rename refuses to overwrite, allows a case-only change.
    create_file(root, "b.txt").unwrap();
    assert_eq!(
        rename(&file, "b.txt").unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let upper = rename(&file, "A.txt").unwrap();
    assert_eq!(upper, root.join("A.txt"));
    assert_eq!(rename(&upper, "A.txt").unwrap(), upper);

    let moved = move_into(&upper, &sub).unwrap();
    assert_eq!(moved, sub.join("A.txt"));
    assert!(move_into(&sub, &sub.join("deeper")).is_err());
    create_file(root, "A.txt").unwrap();
    assert!(move_into(&root.join("A.txt"), &sub).is_err());

    delete_permanently(&sub).unwrap();
    assert!(!sub.exists());
    delete_permanently(&root.join("b.txt")).unwrap();
    assert!(delete_permanently(&root.join("b.txt")).is_err());
}

#[test]
fn renamed_paths_follow_their_folder() {
    let from = Path::new("/p/src");
    let to = Path::new("/p/lib");
    assert_eq!(renamed_path(from, from, to), Some(PathBuf::from("/p/lib")));
    assert_eq!(
        renamed_path(Path::new("/p/src/a/b.rs"), from, to),
        Some(PathBuf::from("/p/lib/a/b.rs"))
    );
    assert_eq!(renamed_path(Path::new("/p/srcx/a.rs"), from, to), None);
}

#[cfg(unix)]
#[test]
fn file_uris_are_encoded() {
    assert_eq!(
        file_uri(Path::new("/home/me/a b#.rs")),
        "file:///home/me/a%20b%23.rs"
    );
}

#[test]
fn verbatim_prefixes_are_stripped_for_other_programs() {
    assert_eq!(
        strip_verbatim(r"\\?\C:\Users\me\proj\a.rs").as_deref(),
        Some(r"C:\Users\me\proj\a.rs")
    );
    assert_eq!(
        strip_verbatim(r"\\?\UNC\server\share\p").as_deref(),
        Some(r"\\server\share\p")
    );
    assert_eq!(strip_verbatim(r"C:\plain"), None);
    assert_eq!(strip_verbatim("/home/me"), None);
    // Names Win32 cannot express keep the prefix.
    assert_eq!(strip_verbatim(r"\\?\C:\odd.\x"), None);
    assert_eq!(strip_verbatim(r"\\?\Volume{1234}\x"), None);
    if !cfg!(windows) {
        let p = Path::new(r"\\?\C:\x");
        assert_eq!(external_path(p), p);
    }
}

#[test]
fn a_case_only_rename_of_the_same_file_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("readme.md"), "x").unwrap();
    let to = rename(&root.join("readme.md"), "README.md").unwrap();
    assert_eq!(to, root.join("README.md"));
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "x");
    let names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names, ["README.md"]);
}
