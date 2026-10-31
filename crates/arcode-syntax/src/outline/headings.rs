//! Document sections from headings (Markdown): a heading's outline item
//! spans to the next heading of the same or a higher level, so ATX and
//! setext headings nest the same way.

use super::Found;

/// Extends every item with a heading `level` to its section end: the start
/// of the next heading with a level `<=` its own, or `doc_end`.
pub(super) fn extend_sections(found: &mut [Found], doc_end: usize) {
    let mut headings: Vec<usize> = (0..found.len())
        .filter(|&i| found[i].level.is_some())
        .collect();
    headings.sort_by_key(|&i| found[i].item.byte_range.start);
    // Headings whose section is still open, levels strictly increasing.
    let mut open: Vec<(usize, u8)> = Vec::new();
    for index in headings {
        let level = found[index].level.unwrap_or(u8::MAX);
        let start = found[index].item.byte_range.start;
        while let Some(&(prev, prev_level)) = open.last() {
            if prev_level < level {
                break;
            }
            found[prev].item.byte_range.end = start.max(found[prev].item.byte_range.start);
            open.pop();
        }
        open.push((index, level));
    }
    for (index, _) in open {
        let range = &mut found[index].item.byte_range;
        range.end = doc_end.max(range.end);
    }
}

/// Drops an ATX closing sequence: `"Sub ##"` becomes `"Sub"`. The hashes
/// must follow a space (`"C#"` stays); a name of only hashes becomes empty.
pub(super) fn strip_atx_closing(name: &str) -> &str {
    let name = name.trim_end();
    let without = name.trim_end_matches('#');
    if without.len() == name.len() {
        name
    } else if without.is_empty() {
        ""
    } else if without.ends_with([' ', '\t']) {
        without.trim_end()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OutlineItem, OutlineKind};

    fn heading(start: usize, end: usize, level: u8) -> Found {
        Found {
            pattern: 0,
            level: Some(level),
            item: OutlineItem {
                name: "h".into(),
                kind: OutlineKind::Heading,
                range: 0..0,
                depth: 0,
                byte_range: start..end,
                name_range: start..end,
            },
        }
    }

    #[test]
    fn sections_end_at_same_or_higher_level() {
        let mut found = vec![
            heading(0, 5, 1),
            heading(10, 15, 2),
            heading(20, 25, 3),
            heading(30, 35, 2),
            heading(40, 45, 1),
        ];
        extend_sections(&mut found, 100);
        let ends: Vec<usize> = found.iter().map(|f| f.item.byte_range.end).collect();
        assert_eq!(ends, [40, 30, 30, 40, 100]);
    }

    #[test]
    fn closing_sequence_is_stripped() {
        assert_eq!(strip_atx_closing("Sub ##"), "Sub");
        assert_eq!(strip_atx_closing("Sub ##  "), "Sub");
        assert_eq!(strip_atx_closing("C#"), "C#");
        assert_eq!(strip_atx_closing("Sub \\#"), "Sub \\#");
        assert_eq!(strip_atx_closing("###"), "");
        assert_eq!(strip_atx_closing("Plain"), "Plain");
    }
}
