//! Large-buffer performance: building and editing must not copy the text.

use std::time::{Duration, Instant};

use arcode_buffer::ops::{self, OpsConfig};
use arcode_buffer::{Buffer, PositionEncoding, SelectionSet, search};

fn big_text() -> String {
    (0..100_000)
        .map(|i| format!("fn line_{i}() {{ let value = {i} * 2; /* ünïcode 😀 */ }}\n"))
        .collect()
}

#[test]
fn hundred_k_lines_thousand_edits() {
    let text = big_text();
    assert!(text.len() > 5_000_000);
    let start = Instant::now();
    let mut b = Buffer::new(&text);
    assert_eq!(b.line_count(), 100_001);

    for i in 0..1000 {
        let line = (i * 97) % 100_000;
        let at = b.line_start(line) + 3;
        let edits = b.edit(vec![(at..at + 4, format!("x{i}"))]);
        assert_eq!(edits.len(), 1);
        let (l, _) = b.to_lsp_position(at, PositionEncoding::Utf16);
        assert_eq!(l as usize, line);
    }
    // Typing through the ops layer with many cursors.
    let mut sels = SelectionSet::single(b.line_end(50_000));
    let cfg = OpsConfig::default();
    for _ in 0..200 {
        ops::type_text(&mut b, &mut sels, "z", &cfg);
    }
    assert!(b.line_text(50_000).ends_with(&"z".repeat(200)));
    let elapsed = start.elapsed();
    // Generous bound so slow CI machines (and debug builds) pass; a
    // per-edit full copy would take far longer.
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");

    // Undo everything back to the original.
    while b.undo().is_some() {}
    assert!(!b.is_dirty());
    assert_eq!(b.len_bytes(), text.len());
    assert_eq!(b.line_text(99_999), text.lines().nth(99_999).expect("line"));
}

#[test]
fn search_in_large_buffer() {
    let b = Buffer::new(&big_text());
    let start = Instant::now();
    let hit = search::find_next(&b, "line_99999()", 0, true, false).expect("found");
    assert_eq!(b.char_to_point(hit.start).line, 99_999);
    let back = search::find_prev(&b, "LINE_0()", b.len_chars(), false, false).expect("found");
    assert_eq!(back.start, 3);
    assert!(start.elapsed() < Duration::from_secs(20));
}
