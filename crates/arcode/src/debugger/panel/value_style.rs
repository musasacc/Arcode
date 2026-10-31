//! Syntax colours for debugger values (docs/DESIGN.md 6.8): numbers and
//! booleans in `number`, strings in `string`, type names in `type`, field
//! names in `property`. A small tokenizer over the adapter's display
//! string, so `Rect { w: 3, h: 4 }` reads like code without a parser.

use std::ops::Range;

use arcode_ui_kit::theme;
use gpui::{App, HighlightStyle, SharedString, StyledText};

/// What a span of a value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueRole {
    /// `3`, `0x1f`, `1.5`, `true`.
    Number,
    /// `"oops"`, `'a'`.
    String,
    /// `Rect`, `Vec`.
    Type,
    /// `w` in `w: 3`.
    Property,
    /// `{`, `,`, `=`.
    Punctuation,
}

impl ValueRole {
    fn capture(self) -> &'static str {
        match self {
            ValueRole::Number => "number",
            ValueRole::String => "string",
            ValueRole::Type => "type",
            ValueRole::Property => "property",
            ValueRole::Punctuation => "punctuation",
        }
    }
}

/// The coloured spans of `value` (byte ranges, in order); the rest is
/// plain text.
pub fn value_spans(value: &str) -> Vec<(Range<usize>, ValueRole)> {
    let bytes = value.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    while i < bytes.len() {
        let b = bytes[i];
        let start = i;
        if b == b'"' || b == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != b {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            spans.push((start..i, ValueRole::String));
        } else if b.is_ascii_digit()
            || (b == b'-'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
                && (i == 0 || !ident(bytes[i - 1])))
        {
            i += 1;
            while i < bytes.len() && (ident(bytes[i]) || bytes[i] == b'.') {
                i += 1;
            }
            spans.push((start..i, ValueRole::Number));
        } else if b.is_ascii_alphabetic() || b == b'_' {
            while i < bytes.len() && ident(bytes[i]) {
                i += 1;
            }
            let word = &value[start..i];
            let field = bytes.get(i) == Some(&b':') && bytes.get(i + 1) != Some(&b':');
            let role = match word {
                "true" | "false" => Some(ValueRole::Number),
                _ if field => Some(ValueRole::Property),
                _ if b.is_ascii_uppercase() => Some(ValueRole::Type),
                _ => None,
            };
            if let Some(role) = role {
                spans.push((start..i, role));
            }
        } else if b"{}[](),:;=<>&*".contains(&b) {
            i += 1;
            spans.push((start..i, ValueRole::Punctuation));
        } else {
            // Any other char, whole (UTF-8 safe).
            i += value[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    spans
}

/// `text` with module paths dropped (`demo::shapes::Rect` → `Rect`,
/// `alloc::vec::Vec<i32, alloc::alloc::Global>` → `Vec<i32, Global>`), so
/// types and values fit their column; quoted text is kept as is. A path
/// segment is dropped when it starts with a lowercase letter or `_` and is
/// followed by `::`.
pub fn strip_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            out.push_str(&text[start..i]);
            continue;
        }
        let starts_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if starts_word && (b.is_ascii_lowercase() || b == b'_') {
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if text[j..].starts_with("::") {
                i = j + 2;
                continue;
            }
            out.push_str(&text[i..j]);
            i = j;
            continue;
        }
        let len = text[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&text[i..i + len]);
        i += len;
    }
    out
}

/// `value` as text coloured with the theme's syntax colours.
pub fn styled_value(value: SharedString, cx: &App) -> StyledText {
    let t = theme(cx);
    let highlights: Vec<(Range<usize>, HighlightStyle)> = value_spans(&value)
        .into_iter()
        .map(|(range, role)| (range, t.syntax(role.capture())))
        .collect();
    StyledText::new(value).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ValueRole::*;

    fn roles(value: &str) -> Vec<(&str, ValueRole)> {
        value_spans(value)
            .into_iter()
            .map(|(r, role)| (&value[r], role))
            .collect()
    }

    #[test]
    fn structs_read_like_code() {
        assert_eq!(
            roles("Rect { w: 3, h: 4 }"),
            [
                ("Rect", Type),
                ("{", Punctuation),
                ("w", Property),
                (":", Punctuation),
                ("3", Number),
                (",", Punctuation),
                ("h", Property),
                (":", Punctuation),
                ("4", Number),
                ("}", Punctuation),
            ]
        );
    }

    #[test]
    fn scalars() {
        assert_eq!(roles("6"), [("6", Number)]);
        assert_eq!(roles("-12"), [("-12", Number)]);
        assert_eq!(roles("0x1f"), [("0x1f", Number)]);
        assert_eq!(roles("1.5e3"), [("1.5e3", Number)]);
        assert_eq!(roles("true"), [("true", Number)]);
        assert_eq!(roles("\"oops\""), [("\"oops\"", String)]);
        assert_eq!(roles("'a'"), [("'a'", String)]);
        assert_eq!(roles(r#""a\"b""#), [(r#""a\"b""#, String)]);
        assert_eq!(roles("\"open"), [("\"open", String)]);
    }

    #[test]
    fn module_paths_are_dropped() {
        assert_eq!(strip_paths("demo::shapes::Rect"), "Rect");
        assert_eq!(
            strip_paths("alloc::vec::Vec<int, alloc::alloc::Global>"),
            "Vec<int, Global>"
        );
        assert_eq!(strip_paths("i32"), "i32");
        assert_eq!(strip_paths("&mut core::option::Option<u8>"), "&mut Option<u8>");
        assert_eq!(strip_paths("\"a::b\""), "\"a::b\"");
        assert_eq!(strip_paths("Rect::new"), "Rect::new");
        assert_eq!(strip_paths("é::x"), "é::x");
    }

    #[test]
    fn paths_and_plain_words() {
        assert_eq!(
            roles("std::vec::Vec<i32>"),
            [
                (":", Punctuation),
                (":", Punctuation),
                (":", Punctuation),
                (":", Punctuation),
                ("Vec", Type),
                ("<", Punctuation),
                (">", Punctuation),
            ]
        );
        assert!(roles("len é x").is_empty());
        // A minus after a word is an operator, not a sign.
        assert_eq!(roles("a-1"), [("1", Number)]);
    }
}
