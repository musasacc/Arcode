use super::*;

use StatusCode::*;

const SHA: &str = "1111111111111111111111111111111111111111";

fn ordinary_record(xy: &str, path: &str) -> String {
    format!("1 {xy} N... 100644 100644 100644 {SHA} {SHA} {path}\0")
}

#[test]
fn parses_ordinary_untracked_and_ignored() {
    let out = format!(
        "# branch.oid {SHA}\0{}{}? new file.txt\0! target/\0",
        ordinary_record(".M", "src/a b.rs"),
        ordinary_record("A.", "added.rs"),
    );
    let entries = parse_porcelain_v2(out.as_bytes()).unwrap();
    assert_eq!(entries.len(), 4);
    let path: PathBuf = ["src", "a b.rs"].iter().collect();
    assert_eq!(entries[0].path, path);
    assert_eq!(
        (entries[0].index, entries[0].worktree),
        (Unmodified, Modified)
    );
    assert!(entries[0].is_unstaged() && !entries[0].is_staged());
    assert_eq!((entries[1].index, entries[1].worktree), (Added, Unmodified));
    assert_eq!(entries[2].path, PathBuf::from("new file.txt"));
    assert_eq!(entries[2].index, Untracked);
    assert_eq!(entries[3].path, PathBuf::from("target"));
    assert_eq!(entries[3].worktree, Ignored);
}

#[test]
fn parses_rename_with_original_path() {
    let out = format!("2 R. N... 100644 100644 100644 {SHA} {SHA} R100 new name.rs\0old.rs\0");
    let entries = parse_porcelain_v2(out.as_bytes()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].index, Renamed);
    assert_eq!(entries[0].path, PathBuf::from("new name.rs"));
    assert_eq!(entries[0].orig_path, Some(PathBuf::from("old.rs")));
}

#[test]
fn parses_unmerged() {
    let out = format!("u UU N... 100644 100644 100644 100644 {SHA} {SHA} {SHA} c.txt\0");
    let entries = parse_porcelain_v2(out.as_bytes()).unwrap();
    assert_eq!(entries[0].conflict, Some(ConflictKind::BothModified));
    assert_eq!(entries[0].index, Unmerged);
    assert!(entries[0].is_conflicted());
    let sum = StatusSummary::from_entries(&entries);
    assert_eq!(sum.conflicted, 1);
    assert!(!sum.is_clean());
}

#[test]
fn rejects_truncated_records() {
    assert!(parse_porcelain_v2(b"1 .M N... 100644\0").is_err());
    assert!(parse_porcelain_v2(b"? \0").is_err());
    let rename = format!("2 R. N... 100644 100644 100644 {SHA} {SHA} R100 new.rs\0");
    assert!(parse_porcelain_v2(rename.as_bytes()).is_err());
    assert!(parse_porcelain_v2(b"").unwrap().is_empty());
}

#[test]
fn summary_counts_both_sides() {
    let out = format!(
        "{}{}? u.txt\0",
        ordinary_record("MM", "both.rs"),
        ordinary_record("D.", "gone.rs"),
    );
    let sum = StatusSummary::from_entries(&parse_porcelain_v2(out.as_bytes()).unwrap());
    assert_eq!(
        sum,
        StatusSummary {
            staged: 2,
            unstaged: 1,
            untracked: 1,
            conflicted: 0,
            ignored: 0,
        }
    );
    assert!(StatusSummary::default().is_clean());
}
