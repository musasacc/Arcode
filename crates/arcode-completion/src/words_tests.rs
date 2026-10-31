use super::*;
use arcode_buffer::Buffer;

fn labels(c: Vec<CompletionCandidate>) -> Vec<String> {
    c.into_iter().map(|c| c.label).collect()
}

#[test]
fn extracts_identifiers() {
    let words: Vec<_> = words_in("let foo_bar = x1 + _tmp; 3abc größe ab über_9 z_").collect();
    assert_eq!(words, ["let", "foo_bar", "_tmp", "größe", "über_9"]);
}

#[test]
fn counts_and_frequency_order() {
    let idx = WordIndex::from_text("alpha beta\nalpha gamma\nalpha beta");
    assert_eq!(idx.count("alpha"), 3);
    assert_eq!(idx.count("beta"), 2);
    assert_eq!(idx.len(), 3);
    assert_eq!(idx.line_count(), 3);
    assert_eq!(
        labels(idx.candidates_for("", None)),
        ["alpha", "beta", "gamma"]
    );
    let c = idx.candidates_for("a", None);
    assert_eq!(c[0].kind, CandidateKind::Word);
    assert_eq!(c[0].source, CandidateSource::BufferWord);
}

#[test]
fn prefix_filter_is_fuzzy_case_insensitive() {
    let idx = WordIndex::from_text("HashMap hash_set other");
    assert_eq!(labels(idx.candidates_for("hm", None)), ["HashMap"]);
    let mut got = labels(idx.candidates_for("HAS", None));
    got.sort();
    assert_eq!(got, ["HashMap", "hash_set"]);
}

#[test]
fn excludes_word_being_typed() {
    let idx = WordIndex::from_text("foobar foob");
    assert_eq!(labels(idx.candidates_for("foob", Some("foob"))), ["foobar"]);
    // A word that also occurs elsewhere is kept.
    let idx = WordIndex::from_text("foob foobar foob");
    assert_eq!(idx.candidates_for("foob", Some("foob")).len(), 2);
}

#[test]
fn update_lines_replaces_range() {
    let mut idx = WordIndex::from_text("one two\nthree four\nfive six");
    idx.update_lines(1..2, "seven\neight");
    assert_eq!(idx.line_count(), 4);
    assert_eq!(idx.count("three"), 0);
    assert_eq!(idx.count("seven"), 1);
    assert_eq!(idx.count("eight"), 1);
    assert_eq!(idx.count("five"), 1);
    // Pure insertion at the end, clamped ranges.
    idx.update_lines(10..20, "tail");
    assert_eq!(idx.line_count(), 5);
    assert_eq!(idx.count("tail"), 1);
    idx.index_text("");
    assert!(idx.is_empty());
    assert_eq!(idx.line_count(), 1);
}

#[test]
fn apply_edits_tracks_buffer() {
    let mut buffer = Buffer::new("alpha beta\ngamma\ndelta epsilon\n");
    let mut idx = WordIndex::from_text(&buffer.to_string());
    // Multi-edit batch: one inserting a line, one deleting across lines.
    let edits = buffer.edit(vec![
        (0..5, "omega\nsigma".to_string()),
        (11..23, String::new()),
    ]);
    idx.apply_edits(&edits, &buffer);
    let fresh = WordIndex::from_text(&buffer.to_string());
    assert_eq!(idx.line_count(), fresh.line_count());
    for w in [
        "omega", "sigma", "beta", "alpha", "gamma", "delta", "epsilon",
    ] {
        assert_eq!(
            idx.count(w),
            fresh.count(w),
            "{w} in {:?}",
            buffer.to_string()
        );
    }
    // Undo-style replace of everything.
    let len = buffer.len_chars();
    let edits = buffer.edit(vec![(0..len, "zeta".to_string())]);
    idx.apply_edits(&edits, &buffer);
    assert_eq!(idx.len(), 1);
    assert_eq!(idx.count("zeta"), 1);
}

#[test]
fn word_at_cursor() {
    assert_eq!(word_at("let foo_bar = 1", 6), Some((4, "foo_bar".into())));
    assert_eq!(word_at("let foo", 7), Some((4, "foo".into())));
    assert_eq!(word_at("a + b", 2), None);
    assert_eq!(word_at("größe", 99), Some((0, "größe".into())));
}
