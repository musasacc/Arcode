//! Pure parts of the find bar: match positions, choosing the current match
//! and turning replacements into editor edits.

use std::ops::Range;

use arcode_buffer::Point;
use arcode_search::{LineMatch, SearchQuery, line_replacements};

/// Buffer ranges of the matches (char columns of the uncut lines).
pub fn match_ranges(lines: &[LineMatch]) -> Vec<Range<Point>> {
    lines
        .iter()
        .flat_map(|l| {
            l.columns
                .iter()
                .map(move |c| Point::new(l.line, c.start)..Point::new(l.line, c.end))
        })
        .collect()
}

/// The first match starting at or after `at`, wrapping to the first one.
pub fn first_at_or_after(matches: &[Range<Point>], at: Point) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.partition_point(|m| m.start < at) % matches.len())
}

/// The match after (or before) `current`, wrapping around. Without a
/// current match it starts from `cursor`.
pub fn step(
    matches: &[Range<Point>],
    current: Option<usize>,
    cursor: Point,
    forward: bool,
) -> Option<usize> {
    let n = matches.len();
    if n == 0 {
        return None;
    }
    match (current, forward) {
        (Some(ix), true) => Some((ix + 1) % n),
        (Some(ix), false) => Some((ix + n - 1) % n),
        (None, true) => first_at_or_after(matches, cursor),
        (None, false) => {
            let before = matches.partition_point(|m| m.start < cursor);
            Some((before + n - 1) % n)
        }
    }
}

/// Edits replacing every match on the given lines. `line_text(line)`
/// returns a line without its break. Matches are found again per line, so
/// regex groups expand exactly as in a project replace.
pub fn replace_edits(
    lines: impl IntoIterator<Item = usize>,
    line_text: impl Fn(usize) -> String,
    query: &SearchQuery,
    replacement: &str,
) -> Vec<(Range<Point>, String)> {
    let mut edits = Vec::new();
    let mut last = None;
    for line in lines {
        if last == Some(line) {
            continue;
        }
        last = Some(line);
        let text = line_text(line);
        let Ok(found) = line_replacements(&text, query, replacement) else {
            return Vec::new();
        };
        edits.extend(
            found
                .into_iter()
                .map(|(r, new)| (Point::new(line, r.start)..Point::new(line, r.end), new)),
        );
    }
    edits
}

/// The edit replacing exactly the match `range` (which must still be a
/// match on its line), or `None`.
pub fn replace_one_edit(
    range: &Range<Point>,
    line_text: &str,
    query: &SearchQuery,
    replacement: &str,
) -> Option<(Range<Point>, String)> {
    let found = line_replacements(line_text, query, replacement).ok()?;
    found
        .into_iter()
        .find(|(r, _)| r.start == range.start.column && r.end == range.end.column)
        .map(|(_, new)| (range.clone(), new))
}

/// `"3 of 12"`, `"No results"`, or `"20000+"` style status text.
pub fn status_text(current: Option<usize>, total: usize, capped: bool) -> String {
    let total_text = if capped {
        format!("{total}+")
    } else {
        total.to_string()
    };
    match (total, current) {
        (0, _) => "No results".to_owned(),
        (_, Some(ix)) => format!("{} of {total_text}", ix + 1),
        (_, None) => format!("{total_text} found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_search::search_text;

    fn p(line: usize, column: usize) -> Point {
        Point::new(line, column)
    }

    fn ranges(text: &str, q: &SearchQuery) -> Vec<Range<Point>> {
        match_ranges(&search_text(text, q).unwrap())
    }

    #[test]
    fn matches_and_current_selection() {
        let q = SearchQuery::new("ab");
        let m = ranges("ab xab\nnone\näab", &q);
        assert_eq!(
            m,
            vec![p(0, 0)..p(0, 2), p(0, 4)..p(0, 6), p(2, 1)..p(2, 3)]
        );
        assert_eq!(first_at_or_after(&m, p(0, 1)), Some(1));
        assert_eq!(first_at_or_after(&m, p(2, 2)), Some(0), "wraps");
        assert_eq!(step(&m, Some(2), p(0, 0), true), Some(0));
        assert_eq!(step(&m, Some(0), p(0, 0), false), Some(2));
        assert_eq!(step(&m, None, p(1, 0), false), Some(1));
        assert_eq!(step(&[], None, p(0, 0), true), None);
    }

    #[test]
    fn replacements_become_edits() {
        let q = SearchQuery {
            is_regex: true,
            ..SearchQuery::new(r"(\d+)")
        };
        let lines = ["a1 b22", "x", "3"];
        let edits = replace_edits([0, 0, 2], |l| lines[l].to_owned(), &q, "<$1>");
        assert_eq!(
            edits,
            vec![
                (p(0, 1)..p(0, 2), "<1>".to_owned()),
                (p(0, 4)..p(0, 6), "<22>".to_owned()),
                (p(2, 0)..p(2, 1), "<3>".to_owned()),
            ]
        );
        let one = replace_one_edit(&(p(0, 4)..p(0, 6)), lines[0], &q, "[$1]");
        assert_eq!(one, Some((p(0, 4)..p(0, 6), "[22]".to_owned())));
        assert_eq!(
            replace_one_edit(&(p(0, 3)..p(0, 6)), lines[0], &q, ""),
            None
        );
    }

    #[test]
    fn status_texts() {
        assert_eq!(status_text(None, 0, false), "No results");
        assert_eq!(status_text(Some(2), 12, false), "3 of 12");
        assert_eq!(status_text(Some(0), 50, true), "1 of 50+");
        assert_eq!(status_text(None, 4, false), "4 found");
    }
}
