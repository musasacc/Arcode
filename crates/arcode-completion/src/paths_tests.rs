use super::*;
use std::fs;

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src/nested")).expect("mkdir");
    fs::create_dir_all(root.join(".git")).expect("mkdir");
    fs::write(root.join("main.rs"), "").expect("write");
    fs::write(root.join("README.md"), "").expect("write");
    fs::write(root.join(".env"), "").expect("write");
    fs::write(root.join("src/lib.rs"), "").expect("write");
    dir
}

fn labels(c: &[CompletionCandidate]) -> Vec<&str> {
    c.iter().map(|c| c.label.as_str()).collect()
}

#[test]
fn lists_relative_dir_folders_first() {
    let dir = setup();
    let c = path_completions(dir.path(), "./");
    assert_eq!(labels(&c), ["src/", "README.md", "main.rs"]);
    assert_eq!(c[0].kind, CandidateKind::Folder);
    assert_eq!(c[0].filter_text.as_deref(), Some("src"));
    assert_eq!(c[0].insert.text(), "src/");
    assert_eq!(c[1].kind, CandidateKind::File);
    assert_eq!(c[1].source, CandidateSource::Path);
}

#[test]
fn partial_name_and_subdirs() {
    let dir = setup();
    let c = path_completions(dir.path(), "./src/li");
    assert_eq!(labels(&c), ["nested/", "lib.rs"]);
    assert_eq!(path_query("./src/li"), "li");
    let nested = dir.path().join("src/nested");
    let c = path_completions(&nested, "../");
    assert_eq!(labels(&c), ["nested/", "lib.rs"]);
}

#[test]
fn hidden_only_with_dot() {
    let dir = setup();
    let c = path_completions(dir.path(), "./.");
    assert!(labels(&c).contains(&".git/"));
    assert!(labels(&c).contains(&".env"));
    assert!(!labels(&path_completions(dir.path(), "./e")).contains(&".env"));
}

#[test]
fn absolute_paths() {
    let dir = setup();
    let abs = format!("{}/src/", dir.path().display());
    if looks_like_path(&abs) {
        let c = path_completions(Path::new("/nonexistent"), &abs);
        assert_eq!(labels(&c), ["nested/", "lib.rs"]);
    }
}

#[test]
fn not_a_path_or_missing_dir() {
    let dir = setup();
    assert!(path_completions(dir.path(), "src/").is_empty());
    assert!(path_completions(dir.path(), "hello").is_empty());
    assert!(path_completions(dir.path(), "./missing/").is_empty());
}

#[test]
fn home_expansion_does_not_panic() {
    let _ = path_completions(Path::new("."), "~/");
}

#[test]
fn path_detection() {
    assert!(looks_like_path("./a"));
    assert!(looks_like_path("../"));
    assert!(looks_like_path("/usr"));
    assert!(looks_like_path("~/x"));
    assert!(!looks_like_path("a/b"));
    assert!(!looks_like_path("~x"));
    assert_eq!(cfg!(windows), looks_like_path("C:\\Users"));
    assert_eq!(split_typed("./src/ma"), ("./src/", "ma"));
    assert_eq!(split_typed("abc"), ("", "abc"));
}

#[test]
fn detect_in_string_literal() {
    let line = r#"let p = include_str!("./src/ma"); "#;
    let col = line.find("ma\"").map(|i| i + 2).expect("col");
    assert_eq!(
        detect_path_prefix(line, col),
        Some((22, "./src/ma".to_string()))
    );
    // Single quotes and backticks.
    assert_eq!(
        detect_path_prefix("import x from '../li", 20),
        Some((15, "../li".into()))
    );
    assert_eq!(detect_path_prefix("`~/", 3), Some((1, "~/".into())));
}

#[test]
fn detect_rejects_non_paths() {
    // Not in a string.
    assert_eq!(detect_path_prefix("x = ./foo", 9), None);
    // String closed before the cursor.
    assert_eq!(detect_path_prefix(r#"f("./a") + 1"#, 11), None);
    // In a string but not a path.
    assert_eq!(detect_path_prefix(r#"f("hello"#, 8), None);
    // Escaped quote does not close the string.
    assert_eq!(
        detect_path_prefix(r#"f("./a\"b/"#, 10),
        Some((3, r#"./a\"b/"#.into()))
    );
    // Cursor past the end.
    assert_eq!(detect_path_prefix("\"./", 10), None);
    // Unicode before the string counts chars.
    assert_eq!(detect_path_prefix("ä = \"./", 7), Some((5, "./".into())));
}
