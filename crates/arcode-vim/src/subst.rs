//! `:[range]s/pattern/replacement/[flags]`.
//!
//! The pattern is **literal** (no regex). Escapes in the pattern: `\/` (the
//! delimiter), `\\`, `\n` (line break), `\t`, `\c` (ignore case); any
//! other `\x` is `x`. In the replacement: `&` and `\0` insert the match,
//! `\&` a literal `&`, `\n` and `\r` a line break, `\t` a tab, `\\` a
//! backslash. Flags: `g` (all matches per line), `i` / `I` (ignore / match
//! case); `c` and `e` are accepted and ignored. An empty pattern reuses the
//! last search. Any delimiter that is not alphanumeric works (`s#a#b#`).

use crate::normal::status;
use crate::search::all_matches;
use crate::text::{first_non_blank, line_of};
use crate::types::{SearchQuery, VimAction, VimCommand};
use crate::vim::{Ed, Vim};

/// Splits `body` (starting with the delimiter) into up to three parts,
/// turning `\{delim}` into the delimiter and keeping other escapes.
fn split(body: &str) -> Option<Vec<String>> {
    let mut chars = body.chars();
    let delim = chars.next()?;
    let mut parts = Vec::new();
    let mut cur = String::new();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(n) if n == delim => cur.push(n),
                Some(n) => {
                    cur.push('\\');
                    cur.push(n);
                }
                None => cur.push('\\'),
            }
        } else if c == delim && parts.len() < 2 {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    Some(parts)
}

/// Unescapes a pattern; returns it and whether `\c` was present.
fn unescape_pattern(raw: &str) -> (String, bool) {
    let mut out = String::new();
    let mut ignore_case = false;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('c') => ignore_case = true,
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    (out, ignore_case)
}

/// The replacement text for one `matched` string.
fn expand(raw: &str, matched: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str(matched),
            '\\' => match chars.next() {
                Some('n' | 'r') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('0') => out.push_str(matched),
                Some(o) => out.push(o),
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

impl Vim {
    /// Runs `:s` with `body` (from the delimiter on) over lines
    /// `first..=last`, as one undo step.
    pub(crate) fn substitute(
        &mut self,
        body: &str,
        first: usize,
        last: usize,
        ed: &mut Ed,
    ) -> VimAction {
        let Some(parts) = split(body) else {
            return status("E486: Pattern not found");
        };
        let (mut pattern, mut ignore_case) = unescape_pattern(&parts[0]);
        let replacement = parts.get(1).cloned().unwrap_or_default();
        let flags = parts.get(2).cloned().unwrap_or_default();
        let global = flags.contains('g');
        if flags.contains('i') {
            ignore_case = true;
        }
        if flags.contains('I') {
            ignore_case = false;
        }
        if pattern.is_empty() {
            match &self.search {
                Some(q) => {
                    pattern = q.pattern.clone();
                    ignore_case |= !q.case_sensitive;
                }
                None => return status("E35: No previous regular expression"),
            }
        }
        self.search = Some(SearchQuery {
            pattern: pattern.clone(),
            whole_word: false,
            case_sensitive: !ignore_case,
            forward: true,
        });
        let range_start = ed.b.line_start(first);
        let range_end = ed.b.line_end(last);
        let mut last_line_seen = None;
        let mut edits = Vec::new();
        for m in all_matches(ed.b, &pattern, !ignore_case) {
            let inside = m.start >= range_start
                && (m.start < range_end || (m.start == range_end && pattern.starts_with('\n')));
            if !inside {
                continue;
            }
            let line = line_of(ed.b, m.start);
            if !global && last_line_seen == Some(line) {
                continue;
            }
            last_line_seen = Some(line);
            let matched = ed.b.slice_to_string(m.clone());
            edits.push((m, expand(&replacement, &matched)));
        }
        if edits.is_empty() {
            return status(&format!("E486: Pattern not found: {pattern}"));
        }
        let replacements = edits.len();
        let mut lines: Vec<usize> = edits.iter().map(|(r, _)| line_of(ed.b, r.start)).collect();
        lines.dedup();
        let out = self.apply_edits(ed, edits);
        if let Some(&(_, e)) = out.last() {
            let p = first_non_blank(ed.b, line_of(ed.b, e));
            self.set_cursors(ed, vec![p]);
        }
        VimAction::Command(VimCommand::Substitute {
            replacements,
            lines: lines.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(
            split("/a/b/g"),
            Some(vec!["a".into(), "b".into(), "g".into()])
        );
        assert_eq!(
            split("#a/b#c#"),
            Some(vec!["a/b".into(), "c".into(), String::new()])
        );
        assert_eq!(split("/a\\/b/c"), Some(vec!["a/b".into(), "c".into()]));
        assert_eq!(split("/x"), Some(vec!["x".into()]));
        assert_eq!(unescape_pattern("a\\nb\\c"), ("a\nb".to_string(), true));
        assert_eq!(expand("[&]\\&\\n", "hi"), "[hi]&\n");
        assert_eq!(expand("a\\\\b", "x"), "a\\b");
    }
}
