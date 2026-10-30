//! Reading Go source headers: package clause and `ignore` build constraints.

/// The package name declared by a Go file (`package main` → `main`).
pub(crate) fn package_name(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("package "))
        .and_then(|l| l.split_whitespace().nth(1))
}

/// Whether the file's build constraints exclude it because of the `ignore`
/// tag (`//go:build ignore`, `// +build ignore`), the usual marker of
/// `go:generate` helper programs.
///
/// Other tags may take any value: the file counts as ignored only when no
/// combination of them satisfies the constraint with `ignore` unset, so
/// platform constraints never exclude a file here.
pub(crate) fn is_build_ignored(text: &str) -> bool {
    let header = text
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with("package "));
    let mut plus_build = Vec::new();
    for line in header {
        if let Some(expr) = line.strip_prefix("//go:build") {
            // `//go:build` supersedes `// +build` lines.
            return unsatisfiable(expr);
        }
        if let Some(options) = line.strip_prefix("// +build") {
            plus_build.push(plus_build_expr(options));
        }
    }
    !plus_build.is_empty() && unsatisfiable(&plus_build.join(" && "))
}

/// `// +build a,!b c` → `((a && !b) || (c))`: options OR, comma terms AND.
fn plus_build_expr(options: &str) -> String {
    let alternatives: Vec<String> = options
        .split_whitespace()
        .map(|option| format!("({})", option.split(',').collect::<Vec<_>>().join(" && ")))
        .collect();
    format!("({})", alternatives.join(" || "))
}

/// Whether `expr` is false for every value of its tags other than `ignore`
/// (which is false). Unparsable or very large expressions count as
/// satisfiable.
fn unsatisfiable(expr: &str) -> bool {
    const MAX_TAGS: usize = 10;
    let Some(tokens) = tokenize(expr) else {
        return false;
    };
    let mut tags: Vec<&str> = tokens
        .iter()
        .map(String::as_str)
        .filter(|t| !matches!(*t, "(" | ")" | "!" | "&&" | "||" | "ignore"))
        .collect();
    tags.sort_unstable();
    tags.dedup();
    if tags.len() > MAX_TAGS {
        return false;
    }
    (0u32..1 << tags.len()).all(|bits| {
        let value = |tag: &str| {
            tags.iter()
                .position(|t| *t == tag)
                .is_some_and(|i| bits & (1 << i) != 0)
        };
        let mut parser = Parser {
            tokens: &tokens,
            pos: 0,
            value: &value,
        };
        let result = parser.or();
        result == Some(false) && parser.pos == tokens.len()
    })
}

fn tokenize(expr: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '(' | ')' | '!' => tokens.push(c.to_string()),
            '&' | '|' => {
                if chars.next() != Some(c) {
                    return None;
                }
                tokens.push(format!("{c}{c}"));
            }
            c if c.is_alphanumeric() || c == '_' || c == '.' => {
                let mut tag = c.to_string();
                while let Some(&n) = chars.peek() {
                    if !(n.is_alphanumeric() || n == '_' || n == '.') {
                        break;
                    }
                    tag.push(n);
                    chars.next();
                }
                tokens.push(tag);
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// Recursive-descent evaluator for `//go:build` expressions.
struct Parser<'a> {
    tokens: &'a [String],
    pos: usize,
    value: &'a dyn Fn(&str) -> bool,
}

impl Parser<'_> {
    fn eat(&mut self, token: &str) -> bool {
        let hit = self.tokens.get(self.pos).is_some_and(|t| t == token);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn or(&mut self) -> Option<bool> {
        let mut value = self.and()?;
        while self.eat("||") {
            value |= self.and()?;
        }
        Some(value)
    }

    fn and(&mut self) -> Option<bool> {
        let mut value = self.not()?;
        while self.eat("&&") {
            value &= self.not()?;
        }
        Some(value)
    }

    fn not(&mut self) -> Option<bool> {
        if self.eat("!") {
            return self.not().map(|v| !v);
        }
        if self.eat("(") {
            let value = self.or()?;
            return self.eat(")").then_some(value);
        }
        let tag = self.tokens.get(self.pos)?;
        if matches!(tag.as_str(), "(" | ")" | "!" | "&&" | "||") {
            return None;
        }
        self.pos += 1;
        Some((self.value)(tag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_clause() {
        assert_eq!(package_name("// doc\npackage main\n"), Some("main"));
        assert_eq!(package_name("x := 1\n"), None);
    }

    #[test]
    fn go_build_ignore() {
        assert!(is_build_ignored("//go:build ignore\n\npackage main\n"));
        assert!(is_build_ignored(
            "// Copyright\n\n//go:build ignore && linux\npackage main\n"
        ));
        assert!(!is_build_ignored("//go:build !ignore\npackage main\n"));
        assert!(!is_build_ignored(
            "//go:build ignore || linux\npackage main\n"
        ));
        assert!(!is_build_ignored(
            "//go:build linux && (amd64 || arm64)\npackage main\n"
        ));
        assert!(!is_build_ignored("package main\n\n//go:build ignore\n"));
        assert!(!is_build_ignored("package main\n"));
        assert!(!is_build_ignored("//go:build !windows\npackage main\n"));
        assert!(is_build_ignored(
            "//go:build (ignore || go1.20) && !go1.20\npackage main\n"
        ));
        assert!(!is_build_ignored("//go:build (((\npackage main\n"));
    }

    #[test]
    fn plus_build_ignore() {
        assert!(is_build_ignored("// +build ignore\n\npackage main\n"));
        assert!(is_build_ignored(
            "// +build linux\n// +build ignore\npackage main\n"
        ));
        assert!(!is_build_ignored("// +build ignore linux\npackage main\n"));
        assert!(is_build_ignored("// +build ignore,linux\npackage main\n"));
        assert!(!is_build_ignored("// +build !ignore\npackage main\n"));
        assert!(!is_build_ignored("// +build !windows\npackage main\n"));
    }
}
