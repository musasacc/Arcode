//! How the text fields of the Settings page read lists: command arguments
//! separated by spaces (with shell-like quotes) and comma-separated items
//! that may be globs.

/// Arguments separated by whitespace, read like a shell does, but
/// without expanding anything: `"…"` and `'…'` keep spaces inside one
/// argument; in `"…"`, `\"` is a quote and `\\` a backslash; outside
/// quotes `\"` and `\'` are quotes. Any other backslash is kept, so
/// Windows paths need no doubling. A quote left open runs to the end.
pub(crate) fn split_args(text: &str) -> Vec<String> {
    #[derive(PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote = Quote::None;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match (&quote, ch) {
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::None,
            (Quote::Single, c) => current.push(c),
            (Quote::Double, '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                current.extend(chars.next());
            }
            (Quote::Double, c) => current.push(c),
            (Quote::None, '\\') if matches!(chars.peek(), Some('"' | '\'')) => {
                current.extend(chars.next());
                started = true;
            }
            (Quote::None, '"') => {
                quote = Quote::Double;
                started = true;
            }
            (Quote::None, '\'') => {
                quote = Quote::Single;
                started = true;
            }
            (Quote::None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (Quote::None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// The arguments as [`split_args`] reads them back: one with spaces or
/// quotes goes in `"…"`, with its `"` (and a `\` before one, or at its
/// end) escaped.
pub(crate) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            let plain = !a.is_empty()
                && !a
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'');
            if plain {
                return a.clone();
            }
            let mut quoted = String::from('"');
            let mut chars = a.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '"' => quoted.push_str("\\\""),
                    '\\' if matches!(chars.peek(), None | Some('"' | '\\')) => {
                        quoted.push_str("\\\\")
                    }
                    c => quoted.push(c),
                }
            }
            quoted.push('"');
            quoted
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Items separated by commas, trimmed, empty ones dropped. A comma inside
/// `{…}` or `[…]` belongs to the item, so a glob such as
/// `*.{min.js,map}` stays one.
pub(crate) fn split_list(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let (mut braces, mut brackets) = (0usize, 0usize);
    for ch in text.chars() {
        match ch {
            '{' => braces += 1,
            '}' => braces = braces.saturating_sub(1),
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            ',' if braces == 0 && brackets == 0 => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    items.push(current);
    items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn arguments_split_on_spaces_and_keep_quoted_ones() {
        assert_eq!(split_args("-l  --login"), ["-l", "--login"]);
        assert_eq!(split_args(r#"-c "echo hi" x"#), ["-c", "echo hi", "x"]);
        assert_eq!(split_args(r#"a """#), ["a", ""]);
        assert!(split_args("   ").is_empty());
        let args = strings(&["-c", "echo hi", ""]);
        assert_eq!(join_args(&args), r#"-c "echo hi" """#);
        assert_eq!(split_args(&join_args(&args)), args);
    }

    #[test]
    fn single_quotes_keep_spaces_too() {
        assert_eq!(split_args("-c 'echo hi'"), ["-c", "echo hi"]);
        assert_eq!(split_args(r#"-c 'say "hi"'"#), ["-c", r#"say "hi""#]);
        assert_eq!(split_args(r#""it's" x"#), ["it's", "x"]);
        assert_eq!(split_args("a'b c'd"), ["ab cd"]);
    }

    #[test]
    fn quotes_inside_an_argument_survive_a_round_trip() {
        let args = strings(&["-c", r#"printf "%s" hi"#]);
        let shown = join_args(&args);
        assert_eq!(shown, r#"-c "printf \"%s\" hi""#);
        assert_eq!(split_args(&shown), args);
        // Adding an argument after it keeps it as it was.
        assert_eq!(
            split_args(&format!("{shown} -l")),
            strings(&["-c", r#"printf "%s" hi"#, "-l"])
        );
        for args in [
            strings(&["it's"]),
            strings(&[r"C:\Program Files\x\"]),
            strings(&[r#"a\"b"#, r"a\\b", r"\", "'", "\"", "a b"]),
            strings(&[r"C:\tools\bin", "--x=1"]),
        ] {
            assert_eq!(split_args(&join_args(&args)), args, "{}", join_args(&args));
        }
        // Windows paths are written as they are.
        assert_eq!(join_args(&strings(&[r"C:\tools\bin"])), r"C:\tools\bin");
        assert_eq!(
            join_args(&strings(&[r"C:\Program Files\x"])),
            r#""C:\Program Files\x""#
        );
        assert_eq!(split_args(r"C:\tools\bin"), [r"C:\tools\bin"]);
        assert_eq!(split_args(r#"\"quoted\""#), [r#""quoted""#]);
    }

    #[test]
    fn lists_split_on_commas_outside_globs() {
        assert_eq!(split_list(" TODO,, FIXME ,"), ["TODO", "FIXME"]);
        assert_eq!(
            split_list("*.{min.js,map}, dist, [ab,c]*.txt"),
            ["*.{min.js,map}", "dist", "[ab,c]*.txt"]
        );
        assert!(split_list(" , ").is_empty());
        let items = strings(&["*.{min.js,map}", "target"]);
        assert_eq!(split_list(&items.join(", ")), items);
    }
}
