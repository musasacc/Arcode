//! Reviewer tests for snippets (expected to fail until fixed).

use std::ops::Range;
use std::time::{Duration, Instant};

use arcode_buffer::Buffer;
use arcode_completion::snippet::{Snippet, SnippetSession, VariableContext, expand};

/// Unclosed `${1:` runs are quadratic: once the depth cap (64) is hit, every
/// further `$` at depth 63 starts a fresh scan to EOF that is never memoized
/// (`failed` is only filled below the cap). 80 KB took ~7 s in release.
#[test]
fn review_unclosed_placeholders_parse_in_linear_time() {
    let src = "${1:".repeat(6000); // 24 KB
    let t = Instant::now();
    let s = Snippet::parse(&src);
    let elapsed = t.elapsed();
    assert_eq!(s.preview_text(), src);
    assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
}

/// Unclosed transform conditionals are quadratic: `read_format_text` scans
/// to EOF for every `$`, and those scans are not memoized. 260 KB took ~21 s
/// in release.
#[test]
fn review_unclosed_transform_conditionals_parse_in_linear_time() {
    let src = "${1/(a)/${1:+".repeat(3000); // 39 KB
    let t = Instant::now();
    let _ = Snippet::parse(&src);
    let elapsed = t.elapsed();
    assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
}

fn start(buffer: &mut Buffer, offset: usize, snippet: &str) -> SnippetSession {
    let e = expand(snippet, &VariableContext::default(), "", "    ");
    buffer.edit(vec![(offset..offset, e.text.clone())]);
    SnippetSession::new(&e, offset)
}

fn type_at(buffer: &mut Buffer, s: &mut SnippetSession, range: Range<usize>, text: &str) {
    let edits = buffer.edit(vec![(range, text.to_string())]);
    let mut updates = s.apply_edits(&edits);
    let mut rounds = 0;
    while !updates.is_empty() && rounds < 10 {
        rounds += 1;
        let edits = buffer.edit(updates);
        updates = s.apply_edits(&edits);
    }
}

/// Adjacent mirrors: typing at the end of the first mirror also counts as
/// typing at the start of the second (both are sticky as the active stop),
/// so the second mirror swallows the insertion and the "sync" then
/// overwrites the first one. Buffer ends as "baa" instead of "abab".
#[test]
fn review_adjacent_mirrors_typing_at_boundary() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "${1:a}$1");
    assert_eq!(b.to_string(), "aa");
    type_at(&mut b, &mut s, 1..1, "b");
    assert_eq!(b.to_string(), "abab");
    assert_eq!(s.active_ranges(), [0..2, 2..4]);
}
