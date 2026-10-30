//! Applying `/regex/format/flags` transforms.

use regex::{Captures, RegexBuilder};

use super::ast::{CaseOp, FormatItem, Transform};

impl Transform {
    /// Applies the transform to `input`. An invalid regex leaves the input
    /// unchanged. Without the `g` flag only the first match is replaced.
    pub fn apply(&self, input: &str) -> String {
        let Ok(re) = RegexBuilder::new(&self.regex)
            .case_insensitive(self.flags.contains('i'))
            .multi_line(self.flags.contains('m'))
            .dot_matches_new_line(self.flags.contains('s'))
            .size_limit(1 << 20)
            .build()
        else {
            return input.to_string();
        };
        let limit = if self.flags.contains('g') { 0 } else { 1 };
        re.replacen(input, limit, |caps: &Captures<'_>| {
            format(&self.format, caps)
        })
        .into_owned()
    }
}

fn group<'a>(caps: &'a Captures<'_>, n: usize) -> &'a str {
    caps.get(n).map_or("", |m| m.as_str())
}

fn format(items: &[FormatItem], caps: &Captures<'_>) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            FormatItem::Text(t) => out.push_str(t),
            FormatItem::Group(n) => out.push_str(group(caps, *n)),
            FormatItem::Case(n, op) => out.push_str(&change_case(group(caps, *n), *op)),
            FormatItem::Conditional {
                group: n,
                if_text,
                else_text,
            } => {
                let matched = !group(caps, *n).is_empty();
                out.push_str(if matched { if_text } else { else_text });
            }
        }
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Applies a case operation.
pub fn change_case(s: &str, op: CaseOp) -> String {
    match op {
        CaseOp::Upcase => s.to_uppercase(),
        CaseOp::Downcase => s.to_lowercase(),
        CaseOp::Capitalize => capitalize(s),
        CaseOp::Pascalcase => s
            .split(|c: char| c == '_' || c == '-' || c.is_whitespace())
            .map(capitalize)
            .collect(),
        CaseOp::Camelcase => {
            let pascal = change_case(s, CaseOp::Pascalcase);
            let mut chars = pascal.chars();
            match chars.next() {
                Some(first) => first.to_lowercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippet::ast::{Node, Snippet};

    fn transform_of(src: &str) -> Transform {
        match Snippet::parse(src).nodes.into_iter().next() {
            Some(Node::Tabstop {
                transform: Some(t), ..
            }) => t,
            other => panic!("not a transform: {other:?}"),
        }
    }

    #[test]
    fn upcase_first_match() {
        let t = transform_of("${1/(.*)/${1:/upcase}/}");
        assert_eq!(t.apply("hello"), "HELLO");
    }

    #[test]
    fn global_flag() {
        let t = transform_of("${1/a/b/g}");
        assert_eq!(t.apply("banana"), "bbnbnb");
        let t = transform_of("${1/a/b/}");
        assert_eq!(t.apply("banana"), "bbnana");
    }

    #[test]
    fn case_insensitive_and_conditionals() {
        let t = transform_of("${1/(x)?(Y)/${1:+has}${1:-no}${2:?two:none}/i}");
        assert_eq!(t.apply("y"), "notwo");
        assert_eq!(t.apply("xy"), "hastwo");
    }

    #[test]
    fn invalid_regex_is_identity() {
        let t = transform_of("${1/(/x/}");
        assert_eq!(t.apply("abc"), "abc");
    }

    #[test]
    fn case_ops() {
        assert_eq!(change_case("foo_bar baz", CaseOp::Pascalcase), "FooBarBaz");
        assert_eq!(change_case("foo-bar", CaseOp::Camelcase), "fooBar");
        assert_eq!(change_case("élan", CaseOp::Capitalize), "Élan");
        assert_eq!(change_case("", CaseOp::Camelcase), "");
    }
}
