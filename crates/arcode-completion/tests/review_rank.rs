//! Reviewer tests for ranking (expected to fail until fixed).

use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate, rank};

fn word(label: &str, sort: Option<&str>) -> CompletionCandidate {
    let mut c = CompletionCandidate::new(label, CandidateKind::Word, CandidateSource::BufferWord);
    c.sort_text = sort.map(str::to_string);
    c
}

/// `match_indices` are documented as char indices into the label. nucleo's
/// `Utf32Str::new` works on grapheme clusters (or raw bytes when every
/// grapheme starts with ASCII), so labels with combining marks get wrong
/// indices, which can even point past the end of the label.
#[test]
fn review_match_indices_are_char_indices_with_combining_marks() {
    // "e" + COMBINING ACUTE ACCENT + "bar": chars e,◌́,b,a,r.
    let label = "e\u{301}bar";
    let items = vec![word(label, None)];
    let ranked = rank("bar", &items, 10);
    assert_eq!(ranked.len(), 1);
    let n = label.chars().count() as u32;
    assert!(
        ranked[0].match_indices.iter().all(|&i| i < n),
        "index out of label range: {:?}",
        ranked[0].match_indices
    );
    assert_eq!(ranked[0].match_indices, vec![2, 3, 4]);
}

/// Same with a non-ASCII base char (NFD file names on macOS, e.g. "café").
#[test]
fn review_match_indices_nfd_non_ascii() {
    // "ü" decomposed: u + COMBINING DIAERESIS, then "ber_x".
    let label = "\u{e9}u\u{308}ber";
    let items = vec![word(label, None)];
    let ranked = rank("ber", &items, 10);
    assert_eq!(ranked.len(), 1);
    // chars: é0 u1 ◌2 b3 e4 r5
    assert_eq!(ranked[0].match_indices, vec![3, 4, 5]);
}

/// Mixing local candidates with and without `sort_text` in one source makes
/// the comparator non-transitive (`Some` vs `None` is `Equal`, then the
/// label length decides, while `Some` vs `Some` compares sort_text). Rust's
/// sort may panic on such comparators ("user-provided comparison function
/// does not correctly implement a total order") and otherwise returns an
/// order that depends on the input permutation.
#[test]
fn review_mixed_sort_text_order_is_consistent() {
    // a: sort "2", len 4; b: none, len 5; c: sort "1", len 6.
    // a<b (len), b<c (len), c<a (sort_text): a cycle.
    let mut items = Vec::new();
    for i in 0..300 {
        let tail = "x".repeat(i % 7);
        let sort = match i % 3 {
            0 => Some(format!("{:03}", 300 - i)),
            1 => None,
            _ => Some(format!("{i:03}")),
        };
        items.push(word(&format!("abc{tail}{i}"), sort.as_deref()));
    }
    let forward = rank("abc", &items, 1000);
    let mut rev = items.clone();
    rev.reverse();
    let backward = rank("abc", &rev, 1000);
    let f: Vec<&str> = forward
        .iter()
        .map(|r| items[r.index].label.as_str())
        .collect();
    let b: Vec<&str> = backward
        .iter()
        .map(|r| rev[r.index].label.as_str())
        .collect();
    assert_eq!(
        f, b,
        "ranking depends on input order (non-total comparator)"
    );
}

#[test]
fn review_minimal_cycle() {
    let items = [
        word("abcd", Some("2")),
        word("abcde", None),
        word("abcdef", Some("1")),
    ];
    let perms: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut results = std::collections::BTreeSet::new();
    for p in perms {
        let v: Vec<_> = p.iter().map(|&i| items[i].clone()).collect();
        let r = rank("abc", &v, 10);
        let labels: Vec<String> = r.iter().map(|r| v[r.index].label.clone()).collect();
        results.insert(labels);
    }
    assert_eq!(
        results.len(),
        1,
        "order depends on input order: {results:?}"
    );
}

/// The empty-query path (`rank_empty`, stable sort) has the same
/// non-transitive comparator and panics too.
#[test]
fn review_empty_query_mixed_sort_text_does_not_panic() {
    let mut items = Vec::new();
    for i in 0..300 {
        let tail = "x".repeat(i % 7);
        let sort = match i % 3 {
            0 => Some(format!("{:03}", 300 - i)),
            1 => None,
            _ => Some(format!("{i:03}")),
        };
        items.push(word(&format!("abc{tail}{i}"), sort.as_deref()));
    }
    let ranked = std::panic::catch_unwind(|| rank("", &items, 1000));
    assert!(ranked.is_ok(), "rank(\"\") panicked on a non-total order");
}
