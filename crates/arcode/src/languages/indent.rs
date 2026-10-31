//! Guessing a file's indentation from its content, and converting leading
//! whitespace between tabs and spaces.

use std::ops::Range;

use arcode_buffer::{Point, Rope};

/// Lines looked at when guessing.
const MAX_LINES: usize = 10_000;
/// Indent sizes considered for space indentation, in tie-break order.
const SIZES: [usize; 5] = [4, 2, 8, 3, 6];

/// What a file's content suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentGuess {
    /// Lines are indented with tabs.
    Tabs,
    /// Lines are indented with this many spaces per level.
    Spaces(usize),
}

/// Guesses the indentation of `lines`; `None` when too few lines are
/// indented to tell (the language settings then apply).
pub fn detect<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<IndentGuess> {
    let mut tab_lines = 0usize;
    let mut space_lines = 0usize;
    let mut counts = [0usize; 9];
    let mut previous: Option<usize> = None;
    for line in lines.into_iter().take(MAX_LINES) {
        let line = line.trim_end_matches(['\n', '\r']);
        let rest = line.trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            continue;
        }
        let lead = &line[..line.len() - rest.len()];
        if lead.starts_with('\t') {
            tab_lines += 1;
            previous = None;
            continue;
        }
        let width = lead.len();
        // ` * text` continues a block comment: alignment, not indentation.
        if lead.contains('\t') || (rest.starts_with('*') && width % 2 == 1) {
            continue;
        }
        if width > 0 {
            space_lines += 1;
        }
        if let Some(prev) = previous {
            let delta = width.abs_diff(prev);
            if (1..=8).contains(&delta) {
                counts[delta] += 1;
            }
        }
        previous = Some(width);
    }
    if tab_lines + space_lines < 2 {
        return None;
    }
    if tab_lines > space_lines {
        return Some(IndentGuess::Tabs);
    }
    let best = SIZES.iter().copied().max_by(|a, b| {
        // `max_by` keeps the last of equal elements: compare reversed
        // positions so earlier sizes win ties.
        counts[*a]
            .cmp(&counts[*b])
            .then_with(|| position(*b).cmp(&position(*a)))
    })?;
    (counts[best] > 0).then_some(IndentGuess::Spaces(best))
}

fn position(size: usize) -> usize {
    SIZES.iter().position(|s| *s == size).unwrap_or(usize::MAX)
}

/// [`detect`] on the first lines of a rope.
pub fn detect_rope(rope: &Rope) -> Option<IndentGuess> {
    let lines: Vec<String> = rope.lines().take(MAX_LINES).map(String::from).collect();
    detect(lines.iter().map(String::as_str))
}

/// Edits that rewrite the leading whitespace of every line with tabs
/// (`to_spaces == false`) or spaces, `tab_width` columns per tab. Spaces
/// that do not fill a whole tab stay spaces.
pub fn convert_edits(
    rope: &Rope,
    to_spaces: bool,
    tab_width: usize,
) -> Vec<(Range<Point>, String)> {
    let tab_width = tab_width.max(1);
    let mut edits = Vec::new();
    for (ix, line) in rope.lines().enumerate() {
        let lead: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if lead.is_empty() {
            continue;
        }
        let width = lead.chars().fold(0, |col, c| match c {
            '\t' => col + tab_width - col % tab_width,
            _ => col + 1,
        });
        let new = if to_spaces {
            " ".repeat(width)
        } else {
            format!(
                "{}{}",
                "\t".repeat(width / tab_width),
                " ".repeat(width % tab_width)
            )
        };
        if new != lead {
            let end = lead.chars().count();
            edits.push((Point::new(ix, 0)..Point::new(ix, end), new));
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(text: &str) -> Option<IndentGuess> {
        detect(text.lines())
    }

    #[test]
    fn spaces_and_their_size() {
        let four = "fn a() {\n    if x {\n        y();\n    }\n}\n";
        assert_eq!(guess(four), Some(IndentGuess::Spaces(4)));
        let two = "a:\n  b:\n    c: 1\n  d: 2\n";
        assert_eq!(guess(two), Some(IndentGuess::Spaces(2)));
    }

    #[test]
    fn tabs_win_when_most_lines_use_them() {
        let text = "func a() {\n\tif x {\n\t\ty()\n\t}\n}\n";
        assert_eq!(guess(text), Some(IndentGuess::Tabs));
    }

    #[test]
    fn block_comment_alignment_is_ignored() {
        let text = "/**\n * Doc\n * more\n */\nfn a() {\n    b();\n    c();\n}\n";
        assert_eq!(guess(text), Some(IndentGuess::Spaces(4)));
    }

    #[test]
    fn too_little_to_tell() {
        assert_eq!(guess("one line\n"), None);
        assert_eq!(guess("a\nb\nc\n"), None);
    }

    #[test]
    fn converts_leading_whitespace_both_ways() {
        let rope = Rope::from_str("a\n\tb\n\t\tc\n  d\n");
        let edits = convert_edits(&rope, true, 4);
        assert_eq!(
            edits,
            vec![
                (Point::new(1, 0)..Point::new(1, 1), "    ".to_string()),
                (Point::new(2, 0)..Point::new(2, 2), "        ".to_string()),
            ]
        );
        let rope = Rope::from_str("    a\n      b\n  c\n");
        let edits = convert_edits(&rope, false, 4);
        assert_eq!(
            edits,
            vec![
                (Point::new(0, 0)..Point::new(0, 4), "\t".to_string()),
                (Point::new(1, 0)..Point::new(1, 6), "\t  ".to_string()),
            ]
        );
    }
}
