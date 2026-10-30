//! Verifies `Edit` records by replaying them on plain `String`s, using the
//! char, byte, tree-sitter point and LSP views independently.

use arcode_buffer::{Buffer, Edit, PositionEncoding, TsPoint};

/// Tiny deterministic PRNG (xorshift) so tests need no extra crates.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

const PIECES: &[&str] = &[
    "a", "é", "😀", "中文", "\n", "e\u{301}", "xyz\n", "", "𝄞", " ", "\t",
];

fn ts_point_of(s: &str, byte: usize) -> TsPoint {
    let before = &s[..byte];
    let row = before.matches('\n').count();
    let col = before.rfind('\n').map_or(byte, |i| byte - i - 1);
    TsPoint::new(row, col)
}

fn char_to_byte(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map_or(s.len(), |(b, _)| b)
}

fn lsp_to_byte(s: &str, line: u32, character: u32, enc: PositionEncoding) -> usize {
    let mut offset = 0;
    for (i, l) in s.split('\n').enumerate() {
        if i == line as usize {
            let mut units = 0;
            for (b, c) in l.char_indices() {
                if units >= character as usize {
                    return offset + b;
                }
                units += match enc {
                    PositionEncoding::Utf8 => c.len_utf8(),
                    PositionEncoding::Utf16 => c.len_utf16(),
                    PositionEncoding::Utf32 => 1,
                };
            }
            return offset + l.len();
        }
        offset += l.len() + 1;
    }
    s.len()
}

/// Replays edits via every coordinate system and checks all agree.
fn replay(before: &str, edits: &[Edit], after: &str) {
    let mut by_char = before.to_string();
    let mut by_byte = before.to_string();
    let mut by_lsp = [before.to_string(), before.to_string(), before.to_string()];
    let encs = [
        PositionEncoding::Utf8,
        PositionEncoding::Utf16,
        PositionEncoding::Utf32,
    ];
    for e in edits {
        // Char view.
        let s = char_to_byte(&by_char, e.range.start);
        let t = char_to_byte(&by_char, e.range.end);
        by_char.replace_range(s..t, &e.new_text);
        // Byte + point view.
        assert_eq!(ts_point_of(&by_byte, e.start_byte), e.start_position);
        assert_eq!(ts_point_of(&by_byte, e.old_end_byte), e.old_end_position);
        assert_eq!(e.start_point.line, e.start_position.row);
        assert_eq!(e.old_end_point.line, e.old_end_position.row);
        by_byte.replace_range(e.start_byte..e.old_end_byte, &e.new_text);
        assert_eq!(e.new_end_byte, e.start_byte + e.new_text.len());
        assert_eq!(ts_point_of(&by_byte, e.new_end_byte), e.new_end_position);
        assert_eq!(e.new_end_point.line, e.new_end_position.row);
        // LSP views.
        for (doc, enc) in by_lsp.iter_mut().zip(encs) {
            let r = e.lsp_range(enc);
            let s = lsp_to_byte(doc, r.start.0, r.start.1, enc);
            let t = lsp_to_byte(doc, r.end.0, r.end.1, enc);
            doc.replace_range(s..t, &e.new_text);
        }
    }
    assert_eq!(by_char, after);
    assert_eq!(by_byte, after);
    for doc in &by_lsp {
        assert_eq!(doc, after);
    }
}

#[test]
fn single_edit_fields() {
    let mut b = Buffer::new("ab\n😀cd\n");
    let edits = b.edit(vec![(4..5, "X\nYZ".into())]);
    assert_eq!(b.to_string(), "ab\n😀X\nYZd\n");
    let e = &edits[0];
    assert_eq!(e.range, 4..5);
    assert_eq!(e.start_byte, 7);
    assert_eq!(e.old_end_byte, 8);
    assert_eq!(e.new_end_byte, 11);
    assert_eq!(e.start_position, TsPoint::new(1, 4));
    assert_eq!(e.old_end_position, TsPoint::new(1, 5));
    assert_eq!(e.new_end_position, TsPoint::new(2, 2));
    assert_eq!(e.new_end_char(), 8);
    let r = e.lsp_range(PositionEncoding::Utf16);
    assert_eq!((r.start, r.end), ((1, 2), (1, 3)));
    let r = e.lsp_range(PositionEncoding::Utf8);
    assert_eq!((r.start, r.end), ((1, 4), (1, 5)));
    let r = e.lsp_range(PositionEncoding::Utf32);
    assert_eq!((r.start, r.end), ((1, 1), (1, 2)));
}

#[test]
fn multi_range_any_order_is_atomic() {
    let before = "héllo wörld\n😀 end";
    let mut b = Buffer::new(before);
    let edits = b.edit(vec![
        (12..13, "🎉🎉".into()),
        (0..1, "H".into()),
        (6..11, "there".into()),
        (17..17, "!".into()),
    ]);
    let after = "Héllo there\n🎉🎉 end!";
    assert_eq!(b.to_string(), after);
    assert_eq!(edits.len(), 4);
    replay(before, &edits, after);
    // One undo step.
    let undo = b.undo().expect("undo");
    assert_eq!(b.to_string(), before);
    replay(after, &undo.edits, before);
    assert!(b.undo().is_none());
}

#[test]
fn randomized_edits_replay_through_undo_redo() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut b = Buffer::new("start\n");
    let mut snapshots = vec![b.to_string()];
    for _ in 0..300 {
        let len = b.len_chars();
        let n = 1 + rng.below(4);
        let mut points: Vec<usize> = (0..n * 2).map(|_| rng.below(len + 1)).collect();
        points.sort_unstable();
        let mut batch = Vec::new();
        for pair in points.chunks(2) {
            let text: String = (0..rng.below(3))
                .map(|_| PIECES[rng.below(PIECES.len())])
                .collect();
            batch.push((pair[0]..pair[1], text));
        }
        // Shuffle order: the API accepts any order.
        batch.reverse();
        let before = b.to_string();
        let edits = b.edit(batch);
        let after = b.to_string();
        replay(&before, &edits, &after);
        if !edits.is_empty() {
            snapshots.push(after);
        }
    }
    // Undo everything, checking each intermediate state and replay.
    let mut idx = snapshots.len() - 1;
    while let Some(res) = b.undo() {
        let before = snapshots[idx].clone();
        idx -= 1;
        assert_eq!(b.to_string(), snapshots[idx]);
        replay(&before, &res.edits, &snapshots[idx]);
    }
    assert_eq!(idx, 0);
    while let Some(res) = b.redo() {
        let before = snapshots[idx].clone();
        idx += 1;
        assert_eq!(b.to_string(), snapshots[idx]);
        replay(&before, &res.edits, &snapshots[idx]);
    }
    assert_eq!(idx, snapshots.len() - 1);
}

#[test]
fn surrogate_pairs_in_lsp_ranges() {
    let before = "𝄞😀x\n";
    let mut b = Buffer::new(before);
    let edits = b.edit(vec![(1..2, "ab".into())]);
    let r = edits[0].lsp_range(PositionEncoding::Utf16);
    assert_eq!((r.start, r.end), ((0, 2), (0, 4)));
    replay(before, &edits, "𝄞abx\n");
}

#[test]
fn noop_edits_do_nothing() {
    let mut b = Buffer::new("abc");
    assert!(b.edit(vec![]).is_empty());
    assert!(b.edit(vec![(1..1, String::new())]).is_empty());
    assert_eq!(b.version(), 0);
    assert!(!b.can_undo());
    assert!(!b.is_dirty());
}

#[test]
fn out_of_range_and_overlapping_are_sanitized() {
    let mut b = Buffer::new("abcdef");
    let edits = b.edit(vec![
        (4..100, "Z".into()),
        (1..3, "x".into()),
        (2..5, "y".into()),
    ]);
    // 1..3 and 2..5 merge into 1..5 ("xy"), then 4..100 overlaps too.
    assert_eq!(b.to_string(), "axyZ");
    replay("abcdef", &edits, "axyZ");
}
