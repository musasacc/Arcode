//! Finding TODO comments in a document's text.
//!
//! This is plain Rust with no host calls, which makes it the easiest part
//! to unit-test. Keeping logic like this apart from the `Plugin` impl is a
//! good habit: most of a plugin can then be tested without any fakes.

/// Lenses stop at this many TODOs per document, so a huge file cannot
/// spend the 100 ms lens budget on building lenses.
pub const MAX_TODOS: usize = 500;

/// Longest TODO text kept (in characters); longer text is cut with `…`.
const MAX_TEXT: usize = 80;

/// Comment markers of the languages the manifest asks lenses for.
const COMMENT_MARKERS: [&str; 5] = ["//", "#", "/*", "<!--", "--"];

/// One TODO comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// Zero-based line number, as lenses and editor positions use.
    pub line: u32,
    /// The comment text from `TODO` to the end of the line, trimmed.
    pub text: String,
}

/// Every TODO comment in `text`, top to bottom, at most [`MAX_TODOS`].
pub fn find(text: &str) -> Vec<Todo> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let line_no = u32::try_from(i).ok()?;
            todo_in_line(line).map(|text| Todo {
                line: line_no,
                text,
            })
        })
        .take(MAX_TODOS)
        .collect()
}

/// The TODO text of one line, if the line has a TODO inside a comment.
pub fn todo_in_line(line: &str) -> Option<String> {
    let at = word_position(line, "TODO")?;
    let before = &line[..at];
    if !COMMENT_MARKERS.iter().any(|marker| before.contains(marker)) {
        return None;
    }
    let rest = line[at..]
        .trim_end()
        .trim_end_matches("*/")
        .trim_end_matches("-->");
    Some(cut(rest.trim_end(), MAX_TEXT))
}

/// Byte offset of the first `word` in `line` that is not part of a longer
/// identifier (so `TODOS` and `MY_TODO` do not count).
fn word_position(line: &str, word: &str) -> Option<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word).map(|(at, _)| at).find(|&at| {
        let before_ok = line[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let after_ok = line[at + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_ident(c));
        before_ok && after_ok
    })
}

/// `text` cut to `max` characters, with `…` when something was cut.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_todo_comments_in_common_languages() {
        let text = "\
fn main() {
    // TODO: say hello
    let todo = \"TODO in a string is not a comment\";
}
# TODO(ann) python or shell
/* TODO: block comment */
<!-- TODO: markdown -->
";
        assert_eq!(
            find(text),
            vec![
                Todo {
                    line: 1,
                    text: "TODO: say hello".into()
                },
                Todo {
                    line: 4,
                    text: "TODO(ann) python or shell".into()
                },
                Todo {
                    line: 5,
                    text: "TODO: block comment".into()
                },
                Todo {
                    line: 6,
                    text: "TODO: markdown".into()
                },
            ]
        );
    }

    #[test]
    fn ignores_longer_words() {
        assert_eq!(todo_in_line("// TODOS are not TODO_ITEMS"), None);
        assert_eq!(todo_in_line("// MY_TODO"), None);
        assert_eq!(todo_in_line("// TODO"), Some("TODO".into()));
    }

    #[test]
    fn long_text_is_cut() {
        let line = format!("// TODO {}", "x".repeat(200));
        let text = todo_in_line(&line).unwrap();
        assert_eq!(text.chars().count(), MAX_TEXT + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn stops_at_the_cap() {
        let text = "// TODO\n".repeat(MAX_TODOS + 10);
        assert_eq!(find(&text).len(), MAX_TODOS);
    }
}
