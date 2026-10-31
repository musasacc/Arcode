//! Parsing TODO-style comments from single lines and in-memory text.

/// Tags recognised when the caller passes none.
pub const DEFAULT_TODO_TAGS: &[&str] = &["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"];

/// Comment closers stripped from the end of a TODO's text.
const CLOSERS: &[&str] = &["*/", "-->", "--]]", "*)", "#}", "%}"];

/// A TODO found in a line of text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TodoMatch {
    /// Zero-based line number.
    pub line: usize,
    /// Char column of the tag.
    pub column: usize,
    /// The tag as spelled in the tag list (e.g. `"FIXME"`).
    pub tag: String,
    /// Text after the tag (and author/colon), trimmed, without trailing
    /// comment closers such as `*/` or `-->`.
    pub text: String,
    /// `name` from `TODO(name): ...`.
    pub author: Option<String>,
}

/// Scan an open buffer's text, case-sensitively. Empty `tags` means
/// [`DEFAULT_TODO_TAGS`]. At most one TODO (the first) per line.
pub fn scan_todos_in_text(text: &str, tags: &[&str]) -> Vec<TodoMatch> {
    let tags = effective_tags(tags);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .enumerate()
        .filter_map(|(n, line)| parse_todo_line(n, line, &tags, true))
        .collect()
}

/// Find the first TODO in `line`. A tag only counts when it is not preceded
/// by a word character and is followed by `:`, `(`, whitespace or the end of
/// the line. Tags may start with a symbol, e.g. `@todo`.
pub fn parse_todo_line(
    line_number: usize,
    line: &str,
    tags: &[&str],
    case_sensitive: bool,
) -> Option<TodoMatch> {
    let mut prev_is_word = false;
    for (column, (at, c)) in line.char_indices().enumerate() {
        // A tag may start with a symbol (`@todo`), but never mid-word.
        let after_word = std::mem::replace(&mut prev_is_word, is_word(c));
        if after_word {
            continue;
        }
        let rest = &line[at..];
        for &tag in tags {
            let Some(len) = tag_prefix_len(rest, tag, case_sensitive) else {
                continue;
            };
            let after = &rest[len..];
            if !after
                .chars()
                .next()
                .is_none_or(|n| n == ':' || n == '(' || n.is_whitespace())
            {
                continue;
            }
            let (author, text) = split_author(after);
            return Some(TodoMatch {
                line: line_number,
                column,
                tag: tag.to_owned(),
                text: clean_text(text),
                author,
            });
        }
    }
    None
}

/// `tags`, or the defaults when empty; empty tags removed, longest first
/// so that e.g. `TODOS` wins over `TODO` when both are listed.
pub(crate) fn effective_tags<'a>(tags: &[&'a str]) -> Vec<&'a str> {
    let mut out: Vec<&str> = if tags.is_empty() {
        DEFAULT_TODO_TAGS.to_vec()
    } else {
        tags.iter().copied().filter(|t| !t.is_empty()).collect()
    };
    out.sort_by_key(|t| std::cmp::Reverse(t.chars().count()));
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte length of `tag` at the start of `hay`, if it is there.
fn tag_prefix_len(hay: &str, tag: &str, case_sensitive: bool) -> Option<usize> {
    if case_sensitive {
        return hay.starts_with(tag).then_some(tag.len());
    }
    let mut hay_chars = hay.char_indices();
    for t in tag.chars() {
        let (_, h) = hay_chars.next()?;
        if h != t && !h.to_lowercase().eq(t.to_lowercase()) {
            return None;
        }
    }
    Some(hay_chars.next().map_or(hay.len(), |(i, _)| i))
}

/// Split `(author)` and an optional `:` off the text after a tag.
fn split_author(after: &str) -> (Option<String>, &str) {
    let mut author = None;
    let mut rest = after;
    if let Some(inner) = rest.strip_prefix('(')
        && let Some(close) = inner.find(')')
    {
        let name = inner[..close].trim();
        if !name.is_empty() {
            author = Some(name.to_owned());
        }
        rest = &inner[close + 1..];
    }
    let rest = rest.trim_start();
    (author, rest.strip_prefix(':').unwrap_or(rest))
}

fn clean_text(text: &str) -> String {
    let mut text = text.trim();
    while let Some(stripped) = CLOSERS.iter().find_map(|c| text.strip_suffix(c)) {
        text = stripped.trim_end();
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Option<TodoMatch> {
        parse_todo_line(0, line, &effective_tags(&[]), true)
    }

    #[test]
    fn basic_forms() {
        let t = one("    // TODO: fix this").unwrap();
        assert_eq!(
            (t.column, t.tag.as_str(), t.text.as_str()),
            (7, "TODO", "fix this")
        );
        assert_eq!(t.author, None);
        let t = one("# FIXME later").unwrap();
        assert_eq!((t.tag.as_str(), t.text.as_str()), ("FIXME", "later"));
        let t = one("/* HACK(ana): workaround */").unwrap();
        assert_eq!(t.author.as_deref(), Some("ana"));
        assert_eq!(t.text, "workaround");
        let t = one("<!-- NOTE: html -->").unwrap();
        assert_eq!(t.text, "html");
        assert_eq!(one("// XXX").unwrap().text, "");
    }

    #[test]
    fn rejects_non_words() {
        assert!(one("let TODOS = 1;").is_none());
        assert!(one("MY_TODO: x").is_none());
        assert!(one("Status::TODO,").is_none());
        assert!(one("todo: lowercase").is_none());
        assert!(one("DEBUG: x").is_none());
    }

    #[test]
    fn case_insensitive_and_unicode_columns() {
        let t = parse_todo_line(4, "äö // todo: x", &["TODO"], false).unwrap();
        assert_eq!((t.line, t.column, t.tag.as_str()), (4, 6, "TODO"));
    }

    #[test]
    fn first_tag_wins_and_author_edge_cases() {
        let t = one("// TODO: fix FIXME later").unwrap();
        assert_eq!(t.tag, "TODO");
        assert_eq!(t.text, "fix FIXME later");
        let t = one("// BUG() empty author").unwrap();
        assert_eq!(t.author, None);
        assert_eq!(t.text, "empty author");
        let t = one("// TODO(unclosed").unwrap();
        assert_eq!(t.text, "(unclosed");
    }

    #[test]
    fn symbol_tags() {
        let tags = ["@todo", "TODO"];
        let t = parse_todo_line(0, "/** @todo(bo): fix */", &tags, true).unwrap();
        assert_eq!((t.column, t.tag.as_str()), (4, "@todo"));
        assert_eq!((t.author.as_deref(), t.text.as_str()), (Some("bo"), "fix"));
        assert!(parse_todo_line(0, "mail me@todo later", &tags, true).is_none());
        assert!(parse_todo_line(0, "x @todos y", &tags, true).is_none());
        let t = parse_todo_line(0, " * @TODO x", &tags, false).unwrap();
        assert_eq!(t.tag, "@todo");
    }

    #[test]
    fn text_scan_lines_and_custom_tags() {
        let text = "\u{feff}a\r\n// TODO: one\r\n// PERF: two\n";
        let items = scan_todos_in_text(text, &[]);
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].line, items[0].text.as_str()), (1, "one"));
        let items = scan_todos_in_text(text, &["PERF"]);
        assert_eq!((items[0].line, items[0].tag.as_str()), (2, "PERF"));
    }
}
