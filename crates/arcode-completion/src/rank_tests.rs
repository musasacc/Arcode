use super::*;
use crate::candidate::CandidateSource;

fn lsp(label: &str) -> CompletionCandidate {
    CompletionCandidate::new(
        label,
        CandidateKind::Function,
        CandidateSource::Lsp {
            server: "ra".into(),
        },
    )
}

fn word(label: &str) -> CompletionCandidate {
    CompletionCandidate::new(label, CandidateKind::Word, CandidateSource::BufferWord)
}

fn labels(query: &str, cands: &[CompletionCandidate]) -> Vec<String> {
    rank(query, cands, 100)
        .into_iter()
        .map(|r| cands[r.index].label.clone())
        .collect()
}

#[test]
fn prefix_beats_fuzzy() {
    let cands = vec![lsp("a_b_c_def"), lsp("abc"), lsp("xabc"), lsp("abcdef")];
    let got = labels("abc", &cands);
    assert_eq!(got[0], "abc");
    assert_eq!(got[1], "abcdef");
    assert!(got.contains(&"xabc".to_string()));
    assert!(got.contains(&"a_b_c_def".to_string()));
}

#[test]
fn non_matching_are_dropped() {
    let cands = vec![lsp("foo"), lsp("bar")];
    assert_eq!(labels("fo", &cands), ["foo"]);
    assert!(labels("zzz", &cands).is_empty());
}

#[test]
fn smart_case() {
    let cands = vec![lsp("hashmap"), lsp("HashMap"), lsp("hash_map_x")];
    // Lower-case query matches all, exact-case prefix first.
    let got = labels("hash", &cands);
    assert_eq!(got.len(), 3);
    assert_eq!(got[2], "HashMap");
    // Upper case in the query makes it case-sensitive.
    assert_eq!(labels("HM", &cands), ["HashMap"]);
    assert_eq!(labels("Hash", &cands), ["HashMap"]);
}

#[test]
fn folded_prefix_beats_fuzzy() {
    let cands = vec![lsp("toString"), lsp("ToStr")];
    // "tostr" is a case-insensitive prefix of both; the shorter wins.
    let got = labels("tostr", &cands);
    assert_eq!(got, ["ToStr", "toString"]);
}

#[test]
fn sort_text_breaks_ties() {
    let mut a = lsp("item_b");
    a.sort_text = Some("2".into());
    let mut b = lsp("item_a");
    b.sort_text = Some("1".into());
    let cands = vec![a, b];
    assert_eq!(labels("item", &cands), ["item_a", "item_b"]);
    let mut cands = cands;
    cands[0].sort_text = Some("0".into());
    assert_eq!(labels("item", &cands), ["item_b", "item_a"]);
}

#[test]
fn source_priority_then_length() {
    let mut snip = word("printx");
    snip.source = CandidateSource::Snippet;
    snip.kind = CandidateKind::Snippet;
    let mut path = word("printy");
    path.source = CandidateSource::Path;
    let cands = vec![
        path,
        word("printz"),
        snip,
        lsp("printw"),
        word("print_long"),
    ];
    let got = labels("print", &cands);
    assert_eq!(got, ["printw", "printx", "printz", "print_long", "printy"]);
}

#[test]
fn shorter_label_wins_last() {
    let cands = vec![word("valueee"), word("value"), word("valuee")];
    assert_eq!(labels("val", &cands), ["value", "valuee", "valueee"]);
}

#[test]
fn empty_query_keeps_lsp_order_preselect_first() {
    let mut a = lsp("zeta");
    a.sort_text = Some("1".into());
    let mut b = lsp("alpha");
    b.sort_text = Some("2".into());
    let mut c = lsp("mid");
    c.sort_text = Some("3".into());
    c.preselect = true;
    let cands = vec![word("wordy"), b, a, c];
    assert_eq!(labels("", &cands), ["mid", "zeta", "alpha", "wordy"]);
    assert!(rank("", &cands, 2).iter().all(|r| r.score == 0));
    assert_eq!(rank("", &cands, 2).len(), 2);
}

#[test]
fn duplicates_merged_keep_best() {
    let mut w = word("render");
    w.kind = CandidateKind::Function;
    let mut l2 = lsp("render");
    l2.source = CandidateSource::Lsp {
        server: "other".into(),
    };
    l2.sort_text = Some("9".into());
    let mut l1 = lsp("render");
    l1.sort_text = Some("1".into());
    let cands = vec![w, l2, l1, lsp("renderer")];
    let ranked = rank("rend", &cands, 10);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].index, 2);
    // Same label but different kind is not a duplicate.
    let cands = vec![lsp("x_y"), word("x_y")];
    assert_eq!(rank("x", &cands, 10).len(), 2);

    let merged = merge_duplicates(vec![word("abc"), lsp("abc"), lsp("abd")]);
    assert_eq!(merged.len(), 3); // different kinds
    let mut w = word("abc");
    w.kind = CandidateKind::Function;
    let mut l = lsp("abc");
    l.detail = Some("fn abc()".into());
    let merged = merge_duplicates(vec![w, lsp("abc"), l, lsp("abd")]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].detail.as_deref(), Some("fn abc()"));
    assert_eq!(merged[1].label, "abd");
}

#[test]
fn match_indices_point_into_label() {
    let cands = vec![lsp("from_str")];
    let r = &rank("fs", &cands, 1)[0];
    assert_eq!(r.match_indices, [0, 5]);
    // filter_text differs: indices are recomputed on the label.
    let mut c = lsp("Vec::new");
    c.filter_text = Some("new".into());
    let r = &rank("new", &[c.clone()], 1)[0];
    assert_eq!(r.match_indices, [5, 6, 7]);
    c.label = "constructor".into();
    let r = &rank("new", &[c], 1)[0];
    assert!(r.match_indices.is_empty());
}

#[test]
fn unicode_labels() {
    let cands = vec![lsp("größe"), lsp("grün")];
    let r = rank("grö", &cands, 10);
    assert_eq!(cands[r[0].index].label, "größe");
    assert_eq!(r[0].match_indices, [0, 1, 2]);
}

#[test]
fn limit_is_respected() {
    let cands: Vec<_> = (0..50).map(|i| lsp(&format!("item{i}"))).collect();
    assert_eq!(rank("item", &cands, 7).len(), 7);
    assert!(rank("item", &cands, 0).is_empty());
}

#[test]
fn ranks_ten_thousand_quickly() {
    let parts = [
        "get", "set", "Buffer", "line", "Count", "to", "str", "map", "Iter", "sel",
    ];
    let cands: Vec<_> = (0..10_000)
        .map(|i| {
            let label = format!(
                "{}_{}{}{}",
                parts[i % 10],
                parts[(i / 10) % 10],
                parts[(i / 100) % 10],
                i
            );
            lsp(&label)
        })
        .collect();
    let mut ranker = Ranker::new();
    let limit = if cfg!(debug_assertions) { 2000 } else { 50 };
    // A selective query and one matching nearly everything.
    for query in ["gsb", "e"] {
        let start = std::time::Instant::now();
        let ranked = ranker.rank(query, &cands, 100);
        let elapsed = start.elapsed();
        println!("ranked 10k candidates for {query:?} in {elapsed:?}");
        assert!(!ranked.is_empty());
        assert!(elapsed.as_millis() < limit, "too slow: {elapsed:?}");
    }
}

#[test]
fn many_duplicates_beyond_partial_sort_head() {
    // 300 copies of one label followed by distinct ones: the dedup must
    // reach past the partially sorted head.
    let mut cands: Vec<_> = (0..300).map(|_| lsp("dup")).collect();
    cands.extend((0..20).map(|i| lsp(&format!("dupe{i:02}"))));
    let ranked = rank("dup", &cands, 10);
    assert_eq!(ranked.len(), 10);
    assert_eq!(cands[ranked[0].index].label, "dup");
    assert_eq!(cands[ranked[1].index].label, "dupe00");
}
