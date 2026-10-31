use super::*;

const HEAD: &str = "diff --git \"a/f.txt\" \"b/f.txt\"\n--- \"a/f.txt\"\n+++ \"b/f.txt\"\n";

fn patch(base: &str, target: &str, old: Range<usize>, new: Range<usize>) -> String {
    let out = hunk_patch("f.txt", base, target, (old, new), &[]).unwrap();
    out.strip_prefix(HEAD).unwrap().to_owned()
}

#[test]
fn modification_with_context() {
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
    let target = "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n";
    assert_eq!(
        patch(base, target, 4..5, 4..5),
        "@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+FIVE\n 6\n 7\n 8\n"
    );
}

#[test]
fn insertion_at_start_of_empty_file() {
    assert_eq!(patch("", "a\nb\n", 0..0, 0..2), "@@ -0,0 +1,2 @@\n+a\n+b\n");
}

#[test]
fn deletion_of_whole_file() {
    assert_eq!(patch("a\nb\n", "", 0..2, 0..0), "@@ -1,2 +0,0 @@\n-a\n-b\n");
}

#[test]
fn appending_after_unterminated_line_rewrites_it() {
    assert_eq!(
        patch("x\na", "x\na\nb", 2..2, 2..3),
        "@@ -1,2 +1,3 @@\n x\n-a\n\\ No newline at end of file\n+a\n+b\n\
         \\ No newline at end of file\n"
    );
}

#[test]
fn unterminated_trailing_context() {
    assert_eq!(
        patch("a\nb\nc", "A\nb\nc", 0..1, 0..1),
        "@@ -1,3 +1,3 @@\n-a\n+A\n b\n c\n\\ No newline at end of file\n"
    );
}

#[test]
fn lone_cr_line_is_widened_to_git_line() {
    // The editor sees three lines; git sees "a\rb\n" and "c\n".
    assert_eq!(
        patch("a\rb\nc\n", "a\rB\nc\n", 1..2, 1..2),
        "@@ -1,2 +1,2 @@\n-a\rb\n+a\rB\n c\n"
    );
}

/// Blocks of `crate::diff_hunks(base, target)`.
fn blocks(base: &str, target: &str) -> Vec<Block> {
    crate::diff_hunks(base, target)
        .iter()
        .map(|h| (h.old_range(), h.new_range()))
        .collect()
}

#[test]
fn widening_absorbs_neighbouring_hunk_of_different_size() {
    // Editor lines: base [A, U, Y], target [B1, B2, U, X]; git sees target
    // as "B1\n" and "B2\rU\rX\n", so hunk 2 shares a git line with hunk 1.
    let (base, target) = ("A\nU\nY\n", "B1\nB2\rU\rX\n");
    let all = blocks(base, target);
    assert_eq!(all, vec![(0..1, 0..2), (2..3, 3..4)]);
    let out = hunk_patch("f.txt", base, target, all[1].clone(), &all).unwrap();
    assert_eq!(
        out.strip_prefix(HEAD).unwrap(),
        "@@ -1,3 +1,2 @@\n-A\n-U\n-Y\n+B1\n+B2\rU\rX\n"
    );
    assert_eq!(
        apply_hunk(base, target, all[1].clone(), &all).unwrap(),
        target
    );
}

#[test]
fn widening_steps_over_unchanged_pairs_only() {
    // Line separator U+2028 joins "u" and "Y" into one git line.
    let (base, target) = ("A\nB\nu\u{2028}Y\n", "A\nC\nD\nu\u{2028}X\n");
    let all = blocks(base, target);
    assert_eq!(all, vec![(1..2, 1..3), (3..4, 4..5)]);
    // Stepping back over the unchanged `u` reaches a git line start;
    // hunk 1 is not absorbed.
    let staged = apply_hunk(base, target, all[1].clone(), &all).unwrap();
    assert_eq!(staged, "A\nB\nu\u{2028}X\n");
    // Hunk 1 alone stays separate: its lines are whole git lines.
    let staged = apply_hunk(base, target, all[0].clone(), &all).unwrap();
    assert_eq!(staged, "A\nC\nD\nu\u{2028}Y\n");
}

#[test]
fn apply_hunk_terminates_unterminated_last_line() {
    let all = blocks("a\nb", "a\nb\nc");
    assert_eq!(
        apply_hunk("a\nb", "a\nb\nc", all[0].clone(), &all).unwrap(),
        "a\nb\nc"
    );
}

#[test]
fn out_of_range_is_stale() {
    assert!(matches!(
        hunk_patch("f", "a\n", "a\n", (3..4, 0..1), &[]),
        Err(GitError::StaleHunk)
    ));
}

#[test]
fn new_file_patch_and_quoting() {
    assert_eq!(
        new_file_patch("d/a b.txt", "x\ny", false),
        "diff --git \"a/d/a b.txt\" \"b/d/a b.txt\"\nnew file mode 100644\n\
         --- /dev/null\n+++ \"b/d/a b.txt\"\n@@ -0,0 +1,2 @@\n+x\n+y\n\
         \\ No newline at end of file\n"
    );
    assert_eq!(quote("a\"b\\c\td\u{1}"), "\"a\\\"b\\\\c\\td\\001\"");
}
