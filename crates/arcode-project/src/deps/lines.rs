//! Where a dependency is declared: zero-based line numbers found by
//! scanning the manifest text (the TOML and JSON parsers keep no spans).
//! Approximate by design; a miss only means the manifest opens at its top.

/// The TOML table header of `line` (`[a.b]` → `a.b`, `[[x]]` → `x`), with
/// quotes and blanks around the dots removed.
pub(crate) fn toml_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('[') {
        return None;
    }
    let inner = t.trim_start_matches('[');
    let end = inner.find(']')?;
    let parts: Vec<String> = split_dotted(&inner[..end]);
    Some(parts.join("."))
}

/// Splits a dotted TOML key, respecting quotes: `target.'cfg(unix)'.x`.
pub(crate) fn split_dotted(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in key.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '.' => parts.push(std::mem::take(&mut cur).trim().to_string()),
            None => cur.push(c),
        }
    }
    parts.push(cur.trim().to_string());
    parts
}

/// The key a `key = value` line assigns (unquoted), if any.
pub(crate) fn toml_key(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let eq = key_end(t)?;
    let key = t[..eq].trim();
    let parts = split_dotted(key);
    parts.into_iter().next().filter(|k| !k.is_empty())
}

/// Byte index of the `=` after a (possibly quoted) key.
fn key_end(t: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in t.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '=' => return Some(i),
            None => {}
        }
    }
    None
}

/// The line of `key` in the TOML table `section` (`key = ...` inside
/// `[section]`, or the header `[section.key]`).
pub(crate) fn toml_key_line(text: &str, section: &str, key: &str) -> Option<usize> {
    let mut current = String::new();
    let sub = format!("{section}.{key}");
    for (ix, line) in text.lines().enumerate() {
        if let Some(h) = toml_header(line) {
            if h == sub {
                return Some(ix);
            }
            current = h;
            continue;
        }
        if current == section && toml_key(line).as_deref() == Some(key) {
            return Some(ix);
        }
    }
    None
}

/// The first line at or after `from` with a quoted string that starts
/// with `name` (compared by `same`), e.g. `"requests>=2"` for `requests`.
pub(crate) fn quoted_item_line(
    text: &str,
    from: usize,
    name: &str,
    same: impl Fn(&str, &str) -> bool,
) -> Option<usize> {
    for (ix, line) in text.lines().enumerate().skip(from) {
        for quote in ['"', '\''] {
            let mut rest = line;
            while let Some(start) = rest.find(quote) {
                let after = &rest[start + 1..];
                let word: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                    .collect();
                if !word.is_empty() && same(&word, name) {
                    return Some(ix);
                }
                let Some(close) = after.find(quote) else {
                    break;
                };
                rest = &after[close + 1..];
            }
        }
    }
    None
}

/// The line of the TOML table header or key `name` (first match).
pub(crate) fn toml_section_line(text: &str, section: &str) -> Option<usize> {
    text.lines()
        .position(|l| toml_header(l).as_deref() == Some(section))
}

/// The line of `"key":` inside the JSON object that follows `"section"`.
pub(crate) fn json_key_line(text: &str, section: &str, key: &str) -> Option<usize> {
    let section_pat = format!("\"{section}\"");
    let key_pat = format!("\"{key}\"");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| {
        l.find(&section_pat)
            .is_some_and(|i| l[i + section_pat.len()..].trim_start().starts_with(':'))
    })?;
    let braces = |s: &str| s.matches('{').count() as i32 - s.matches('}').count() as i32;
    let mut depth = 0i32;
    let mut opened = false;
    for (ix, line) in lines.iter().enumerate().skip(start) {
        let from = if ix == start {
            line.find(&section_pat).map_or(0, |i| i + section_pat.len())
        } else {
            0
        };
        let part = &line[from..];
        if let Some(i) = part.find(&key_pat)
            && part[i + key_pat.len()..].trim_start().starts_with(':')
            && depth + braces(&part[..i]) == 1
        {
            return Some(ix);
        }
        depth += braces(part);
        opened |= depth > 0;
        if opened && depth <= 0 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_lines() {
        let text = "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\n\"tokio\" = { version = \"1\" }\n\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n[dependencies.rand]\nversion = \"0.8\"\n";
        assert_eq!(toml_key_line(text, "dependencies", "serde"), Some(4));
        assert_eq!(toml_key_line(text, "dependencies", "tokio"), Some(5));
        assert_eq!(
            toml_key_line(text, "target.cfg(unix).dependencies", "libc"),
            Some(8)
        );
        assert_eq!(toml_key_line(text, "dependencies", "rand"), Some(9));
        assert_eq!(toml_key_line(text, "dependencies", "name"), None);
        assert_eq!(toml_section_line(text, "dependencies"), Some(3));
    }

    #[test]
    fn json_lines() {
        let text = "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"react\": \"^18\",\n    \"@types/node\": \"20\"\n  },\n  \"devDependencies\": {\n    \"react\": \"^18\"\n  }\n}\n";
        assert_eq!(json_key_line(text, "dependencies", "react"), Some(3));
        assert_eq!(json_key_line(text, "dependencies", "@types/node"), Some(4));
        assert_eq!(json_key_line(text, "devDependencies", "react"), Some(7));
        assert_eq!(json_key_line(text, "dependencies", "vue"), None);
        let one_line = "{\"dependencies\": {\"a\": \"1\"}}";
        assert_eq!(json_key_line(one_line, "dependencies", "a"), Some(0));
    }

    #[test]
    fn quoted_items() {
        let text = "[project]\ndependencies = [\n  \"requests>=2\",\n  'Flask_Login',\n]\n";
        let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        assert_eq!(quoted_item_line(text, 0, "requests", eq), Some(2));
        assert_eq!(
            quoted_item_line(text, 0, "flask_login", |a, b| a.eq_ignore_ascii_case(b)),
            Some(3)
        );
        assert_eq!(quoted_item_line(text, 3, "requests", eq), None);
    }
}
