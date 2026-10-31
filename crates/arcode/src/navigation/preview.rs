//! Preview lines for reference results: the line of each match, trimmed,
//! with the match's byte range in it. Runs on a background thread; open
//! buffers (which may hold unsaved edits the server already knows) are
//! passed in as rope snapshots, other files are read from disk once.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use arcode_buffer::Rope;
use arcode_lsp::PositionEncoding;

use super::item::{NavItem, NavPos};

/// Longest preview kept around the match, in chars.
const MAX_PREVIEW_CHARS: usize = 160;
/// Chars kept before a match that starts far into a long line.
const LEAD_CHARS: usize = 30;
/// Chars a picker row shows; a match ending later scrolls the preview.
const VISIBLE_CHARS: usize = 70;
/// Files larger than this are not read for previews.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Sets each item's label to the preview of its line and `highlight` to
/// the match in it. Items whose file cannot be read keep their label.
pub fn fill_previews(items: &mut [NavItem], open: &HashMap<PathBuf, Rope>) {
    let mut files: HashMap<PathBuf, Option<Rope>> = HashMap::new();
    for item in items {
        let path = item.target.path.clone();
        let rope = files
            .entry(path.clone())
            .or_insert_with(|| open.get(&path).cloned().or_else(|| read(&path)));
        let Some(rope) = rope else {
            continue;
        };
        let line_ix = item.target.line();
        if line_ix >= rope.len_lines() {
            continue;
        }
        let line = rope.line(line_ix).to_string();
        let line = line.trim_end_matches(['\n', '\r']);
        let cols = match &item.target.pos {
            NavPos::Points(r) => {
                let end = if r.end.line == r.start.line {
                    r.end.column
                } else {
                    line.chars().count()
                };
                r.start.column..end
            }
            NavPos::Lsp { range, encoding } => {
                let start = char_column(line, range.start.character, *encoding);
                let end = if range.end.line == range.start.line {
                    char_column(line, range.end.character, *encoding)
                } else {
                    line.chars().count()
                };
                start..end
            }
        };
        let (label, highlight) = preview(line, cols);
        item.label = label;
        item.highlight = Some(highlight);
    }
}

fn read(path: &std::path::Path) -> Option<Rope> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Rope::from_str(&String::from_utf8_lossy(&bytes)))
}

/// The char column `units` code units into `line`.
pub fn char_column(line: &str, units: u32, encoding: PositionEncoding) -> usize {
    let units = units as usize;
    let mut seen = 0;
    for (col, c) in line.chars().enumerate() {
        if seen >= units {
            return col;
        }
        seen += match encoding {
            PositionEncoding::Utf8 => c.len_utf8(),
            PositionEncoding::Utf16 => c.len_utf16(),
            PositionEncoding::Utf32 => 1,
        };
    }
    line.chars().count()
}

/// `line` without its indentation, cut to a window around `cols`, and the
/// byte range of `cols` in the result.
pub fn preview(line: &str, cols: Range<usize>) -> (String, Range<usize>) {
    let chars: Vec<char> = line.chars().collect();
    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    let start_col = cols.start.min(chars.len());
    let end_col = cols.end.clamp(start_col, chars.len());
    let mut from = indent.min(start_col);
    let mut prefix = "";
    if start_col - from > LEAD_CHARS && end_col - from > VISIBLE_CHARS {
        from = start_col - LEAD_CHARS;
        prefix = "…";
    }
    let to = chars
        .len()
        .min(from + MAX_PREVIEW_CHARS)
        .max(end_col.min(chars.len()));
    let body: String = chars[from..to].iter().collect();
    let body = body.trim_end();
    let before: String = chars[from..start_col].iter().collect();
    let matched: String = chars[start_col..end_col].iter().collect();
    let start = prefix.len() + before.len();
    let text = format!("{prefix}{body}");
    let end = (start + matched.len()).min(text.len());
    (text, start.min(end)..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::item::NavTarget;
    use arcode_lsp::lsp_types::{Position, Range as LspRange};

    #[test]
    fn utf16_columns_count_surrogate_pairs() {
        let line = "a😀b";
        assert_eq!(char_column(line, 1, PositionEncoding::Utf16), 1);
        assert_eq!(char_column(line, 3, PositionEncoding::Utf16), 2);
        assert_eq!(char_column(line, 5, PositionEncoding::Utf8), 2);
        assert_eq!(char_column(line, 99, PositionEncoding::Utf32), 3);
    }

    #[test]
    fn previews_drop_indentation_and_mark_the_match() {
        let (text, hl) = preview("    let x = helper(1);", 12..18);
        assert_eq!(text, "let x = helper(1);");
        assert_eq!(&text[hl], "helper");
        let long = format!("{}target{}", "x".repeat(300), "y".repeat(300));
        let (text, hl) = preview(&long, 300..306);
        assert!(text.starts_with('…'));
        assert_eq!(&text[hl], "target");
        assert!(text.chars().count() <= MAX_PREVIEW_CHARS + 1);
        // A match just past the visible part of a medium line is kept in view.
        let line = format!("{}hit();", "a".repeat(80));
        let (text, hl) = preview(&line, 80..83);
        assert!(text.starts_with('…') && hl.start < VISIBLE_CHARS);
    }

    #[test]
    fn open_buffers_win_over_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, "fn a() {}\nfn helper() {}\n").unwrap();
        let target = |line| NavTarget {
            path: path.clone(),
            pos: NavPos::Lsp {
                range: LspRange::new(Position::new(line, 3), Position::new(line, 9)),
                encoding: PositionEncoding::Utf16,
            },
        };
        let mut items = vec![NavItem::new("", target(1))];
        fill_previews(&mut items, &HashMap::new());
        assert_eq!(items[0].label, "fn helper() {}");
        assert_eq!(items[0].highlight, Some(3..9));
        let open = HashMap::from([(path.clone(), Rope::from_str("x\n   fn h_edit() {}\n"))]);
        fill_previews(&mut items, &open);
        assert_eq!(items[0].label, "fn h_edit() {}");
    }
}
