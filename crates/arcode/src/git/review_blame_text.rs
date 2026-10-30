//! Review tests for blame column formatting.

use arcode_git::BlameLine;

use super::blame_text::{COLUMN_WIDTH, column_text};

fn line(author: &str, commit: &str) -> BlameLine {
    BlameLine {
        line: 0,
        commit: commit.into(),
        author: author.into(),
        author_time: 0,
        summary: String::new(),
        is_uncommitted: false,
    }
}

/// Display columns of `text` in a monospace font: East Asian wide
/// characters (CJK ideographs, Hangul, kana) take two cells.
fn cells(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6 => 2,
            _ => 1,
        })
        .sum()
}

/// The blame column is inline text before the code, padded by char count.
/// A CJK author name is twice as wide on screen, so the code of that
/// block is pushed right and no longer lines up with the other lines.
#[test]
fn review_blame_column_has_a_fixed_display_width_for_cjk_authors() {
    let lines = [line("张伟张伟张伟", "aaaaaaa"), line("Bob", "bbbbbbb")];
    let cjk = column_text(&lines, 0, 0);
    let ascii = column_text(&lines, 1, 0);
    assert_eq!(cells(&ascii), COLUMN_WIDTH);
    assert_eq!(
        cells(&cjk),
        cells(&ascii),
        "CJK entry {cjk:?} is wider on screen than {ascii:?}"
    );
}
