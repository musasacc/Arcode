use super::*;

fn change(kind: ChangeKind, new_lines: Range<usize>, old_lines: Range<usize>) -> LineChange {
    LineChange {
        kind,
        new_lines,
        old_lines,
    }
}

use ChangeKind::{Added, Deleted, Modified};

#[test]
fn identical_and_empty() {
    assert!(line_diff("", "").is_empty());
    assert!(line_diff("a\nb\n", "a\nb\n").is_empty());
}

#[test]
fn empty_old_is_all_added() {
    assert_eq!(line_diff("", "a\nb\n"), vec![change(Added, 0..2, 0..0)]);
}

#[test]
fn empty_new_is_one_deletion() {
    assert_eq!(line_diff("a\nb\n", ""), vec![change(Deleted, 0..0, 0..2)]);
}

#[test]
fn pure_insert() {
    assert_eq!(
        line_diff("a\nb\nc\n", "a\nx\ny\nb\nc\n"),
        vec![change(Added, 1..3, 1..1)]
    );
    // Insert at start and at end.
    assert_eq!(line_diff("a\n", "x\na\n"), vec![change(Added, 0..1, 0..0)]);
    assert_eq!(line_diff("a\n", "a\nx\n"), vec![change(Added, 1..2, 1..1)]);
}

#[test]
fn pure_delete_marks_following_line() {
    assert_eq!(
        line_diff("a\nb\nc\nd\n", "a\nd\n"),
        vec![change(Deleted, 1..1, 1..3)]
    );
    // Deleting the first line marks line 0.
    assert_eq!(
        line_diff("a\nb\n", "b\n"),
        vec![change(Deleted, 0..0, 0..1)]
    );
    // Deleting the last line marks one past the end.
    assert_eq!(
        line_diff("a\nb\n", "a\n"),
        vec![change(Deleted, 1..1, 1..2)]
    );
}

#[test]
fn modify() {
    assert_eq!(
        line_diff("a\nb\nc\n", "a\nB\nc\n"),
        vec![change(Modified, 1..2, 1..2)]
    );
    // Two old lines replaced by three new ones.
    assert_eq!(
        line_diff("a\nb\nc\nd\n", "a\nX\nY\nZ\nd\n"),
        vec![change(Modified, 1..4, 1..3)]
    );
}

#[test]
fn mixed() {
    let old = "fn a\nfn b\nfn c\nfn d\nfn e\nfn f\n";
    let new = "fn a\nnew\nfn b\nfn C\nfn d\nfn f\nend\n";
    assert_eq!(
        line_diff(old, new),
        vec![
            change(Added, 1..2, 1..1),
            change(Modified, 3..4, 2..3),
            change(Deleted, 5..5, 4..5),
            change(Added, 6..7, 6..6),
        ]
    );
}

#[test]
fn trailing_newline_differences() {
    // Only the final newline changed: nothing to show.
    assert!(line_diff("a\nb", "a\nb\n").is_empty());
    assert!(line_diff("a\nb\n", "a\nb").is_empty());
    // A real extra empty line is an addition.
    assert_eq!(line_diff("a\n", "a\n\n"), vec![change(Added, 1..2, 1..1)]);
    // Content change on the unterminated last line.
    assert_eq!(
        line_diff("a\nb", "a\nc"),
        vec![change(Modified, 1..2, 1..2)]
    );
}

#[test]
fn crlf_equals_lf() {
    assert!(line_diff("a\r\nb\r\n", "a\nb\n").is_empty());
    assert_eq!(
        line_diff("a\r\nb\r\n", "a\nB\n"),
        vec![change(Modified, 1..2, 1..2)]
    );
}

#[test]
fn whitespace_only_lines_count() {
    assert_eq!(line_diff("a\n", "a \n"), vec![change(Modified, 0..1, 0..1)]);
}

#[test]
fn completely_different_large_inputs_finish() {
    let old: String = (0..20_000).map(|i| format!("old {i}\n")).collect();
    let new: String = (0..20_000).map(|i| format!("new {i}\n")).collect();
    let changes = line_diff(&old, &new);
    assert_eq!(changes, vec![change(Modified, 0..20_000, 0..20_000)]);
}

#[test]
fn large_file_small_edit() {
    let old: String = (0..50_000).map(|i| format!("line {i}\n")).collect();
    let new = old.replacen("line 25000\n", "changed\n", 1);
    assert_eq!(
        line_diff(&old, &new),
        vec![change(Modified, 25_000..25_001, 25_000..25_001)]
    );
}

/// Tiny deterministic PRNG so the property test needs no dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn lcs_len(a: &[&str], b: &[&str]) -> usize {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    dp[0][0]
}

/// Applies `changes` to `old` and checks the result equals `new`, the
/// blocks are ordered and separated, and the script is minimal.
fn check(old: &[&str], new: &[&str], changes: &[LineChange]) {
    let mut rebuilt: Vec<&str> = Vec::new();
    let (mut oi, mut ni) = (0, 0);
    let mut edits = 0;
    for c in changes {
        assert!(
            c.old_lines.start >= oi && c.new_lines.start >= ni,
            "{changes:?}"
        );
        assert_eq!(c.old_lines.start - oi, c.new_lines.start - ni);
        if oi > 0 || ni > 0 {
            assert!(
                c.old_lines.start > oi || c.new_lines.start > ni,
                "adjacent blocks"
            );
        }
        rebuilt.extend(&old[oi..c.old_lines.start]);
        rebuilt.extend(&new[c.new_lines.clone()]);
        assert_eq!(c.kind, kind_of(&c.old_lines, &c.new_lines));
        edits += c.old_lines.len() + c.new_lines.len();
        oi = c.old_lines.end;
        ni = c.new_lines.end;
    }
    rebuilt.extend(&old[oi..]);
    assert_eq!(rebuilt, new);
    assert_eq!(edits, old.len() + new.len() - 2 * lcs_len(old, new));
}

#[test]
fn random_scripts_are_valid_and_minimal() {
    let alphabet = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for round in 0..3_000 {
        // Vary alphabet size and length to hit both dense and sparse matches.
        let symbols = 2 + (round % 7) as u64;
        let max_len = if round % 10 == 0 { 60 } else { 14 };
        let old: Vec<&str> = (0..rng.below(max_len))
            .map(|_| alphabet[rng.below(symbols) as usize])
            .collect();
        let new: Vec<&str> = (0..rng.below(max_len))
            .map(|_| alphabet[rng.below(symbols) as usize])
            .collect();
        let old_text: String = old.iter().map(|l| format!("{l}\n")).collect();
        let new_text: String = new.iter().map(|l| format!("{l}\n")).collect();
        check(&old, &new, &line_diff(&old_text, &new_text));
    }
}

#[test]
fn split_lines_matches_buffer_line_breaks() {
    let lines = |t| split_lines(t).collect::<Vec<_>>();
    assert!(lines("").is_empty());
    assert_eq!(lines("\n"), vec![""]);
    assert_eq!(lines("a"), vec!["a"]);
    assert_eq!(lines("a\nb\n"), vec!["a", "b"]);
    assert_eq!(lines("a\r\nb\rc"), vec!["a", "b", "c"]);
    assert_eq!(lines("\r\r\n"), vec!["", ""]);
    assert_eq!(lines("a\n\nb"), vec!["a", "", "b"]);
    assert_eq!(
        lines("1\u{0B}2\u{0C}3\u{85}4\u{2028}5\u{2029}6"),
        vec!["1", "2", "3", "4", "5", "6"]
    );
    assert_eq!(lines("é\r\n😀\r"), vec!["é", "😀"]);
}

#[test]
fn mixed_line_endings_compare_equal() {
    assert!(line_diff("a\rb\r\nc\n", "a\nb\nc").is_empty());
    let changes = line_diff("a\u{2028}b\nc\n", "a\u{2028}b\nC\n");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].new_lines, 2..3);
}
