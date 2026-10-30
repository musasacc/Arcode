//! A small Markdown subset for documentation popups, rendered as styled
//! text: headings, paragraphs, bullets, rules, fenced code blocks and
//! inline code, bold, italic and links (shown as their text).

use std::ops::Range;

use arcode_ui_kit::{settings, theme};
use gpui::{
    AnyElement, App, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement,
    SharedString, Styled, StyledText, UnderlineStyle, div, px,
};

use super::Documentation;

/// Inline emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineStyle {
    /// `**bold**`
    Bold,
    /// `*italic*` / `_italic_`
    Italic,
    /// `` `code` ``
    Code,
    /// `[text](url)`: only the text is kept.
    Link,
}

/// Text with styled byte ranges (sorted, not overlapping).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    /// The visible text.
    pub text: String,
    /// Styled byte ranges of `text`.
    pub spans: Vec<(Range<usize>, InlineStyle)>,
}

/// A block of a documentation text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `# Heading`
    Heading(Inline),
    /// A paragraph (soft line breaks joined).
    Paragraph(Inline),
    /// `- item`
    Bullet(Inline),
    /// A fenced code block and the language named after its fence
    /// (e.g. `rust` for "```rust"), used to color it.
    Code {
        /// The fence's language tag, if any.
        language: Option<String>,
        /// The code.
        text: String,
    },
    /// `---`
    Rule,
}

/// Parses `src` into blocks.
pub fn parse_markdown(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut lines = src.lines();
    let flush = |para: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            blocks.push(Block::Paragraph(parse_inline(&join_soft(para))));
            para.clear();
        }
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(fence) = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
            flush(&mut para, &mut blocks);
            let tag = trimmed[fence.len()..].trim();
            let language = tag
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            let code: Vec<&str> = lines
                .by_ref()
                .take_while(|l| !l.trim_start().starts_with(fence))
                .collect();
            blocks.push(Block::Code {
                language,
                text: code.join("\n"),
            });
        } else if trimmed.is_empty() {
            flush(&mut para, &mut blocks);
        } else if let Some(rest) = heading(trimmed) {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(parse_inline(rest)));
        } else if is_rule(trimmed) {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Rule);
        } else if let Some(rest) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Bullet(parse_inline(rest.trim())));
        } else {
            para.push(line);
        }
    }
    flush(&mut para, &mut blocks);
    blocks
}

/// Plain text: blank-line separated paragraphs, line breaks kept.
pub(crate) fn parse_plain(src: &str) -> Vec<Block> {
    src.split("\n\n")
        .map(|p| p.trim_matches('\n').trim_end())
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            Block::Paragraph(Inline {
                text: p.to_string(),
                spans: Vec::new(),
            })
        })
        .collect()
}

/// Blocks of a documentation value.
pub(crate) fn blocks_for(doc: &Documentation) -> Vec<Block> {
    match doc {
        Documentation::PlainText(t) => parse_plain(t),
        Documentation::Markdown(t) => parse_markdown(t),
    }
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let rest = line[hashes..].strip_prefix(' ')?;
    (1..=6).contains(&hashes).then(|| rest.trim())
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && ['-', '*', '_'].iter().any(|c| t.chars().all(|x| x == *c))
}

/// Joins paragraph lines: soft breaks become spaces, a trailing backslash
/// or two spaces keep the break.
fn join_soft(lines: &[&str]) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let hard = line.ends_with("  ") || line.ends_with('\\');
        let text = line.trim_end_matches('\\').trim();
        out.push_str(text);
        if i + 1 < lines.len() {
            out.push(if hard { '\n' } else { ' ' });
        }
    }
    out
}

/// Parses inline emphasis, code spans and links.
fn parse_inline(src: &str) -> Inline {
    let mut out = Inline::default();
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        let after = &rest[c.len_utf8()..];
        if c == '\\'
            && let Some(next) = after.chars().next().filter(|n| n.is_ascii_punctuation())
        {
            out.text.push(next);
            rest = &after[next.len_utf8()..];
            continue;
        }
        let marked = match c {
            '`' => delimited(rest, "`", InlineStyle::Code),
            '*' | '_' if after.starts_with(c) => {
                delimited(rest, if c == '*' { "**" } else { "__" }, InlineStyle::Bold)
            }
            '*' => delimited(rest, "*", InlineStyle::Italic),
            '_' if !out.text.ends_with(|p: char| p.is_alphanumeric()) => {
                delimited(rest, "_", InlineStyle::Italic)
            }
            '[' => link(rest),
            _ => None,
        };
        match marked {
            Some((inner, style, used)) => {
                let start = out.text.len();
                out.text.push_str(inner);
                out.spans.push((start..out.text.len(), style));
                rest = &rest[used..];
            }
            None => {
                out.text.push(c);
                rest = after;
            }
        }
    }
    out
}

/// `delim inner delim` at the start of `s`: (inner, style, bytes used).
fn delimited<'a>(
    s: &'a str,
    delim: &str,
    style: InlineStyle,
) -> Option<(&'a str, InlineStyle, usize)> {
    let body = &s[delim.len()..];
    let end = body.find(delim)?;
    (end > 0).then(|| (&body[..end], style, delim.len() * 2 + end))
}

/// `[text](url)` at the start of `s`.
fn link(s: &str) -> Option<(&str, InlineStyle, usize)> {
    let close = s.find("](")?;
    let url_end = s[close..].find(')')? + close;
    (close > 1).then(|| (&s[1..close], InlineStyle::Link, url_end + 1))
}

/// Elements for `blocks` in the popup's text style.
pub(crate) fn render_blocks(blocks: &[Block], cx: &App) -> Vec<AnyElement> {
    let colors = &theme(cx).colors;
    let mono = SharedString::from(settings(cx).editor.font_family.clone());
    blocks
        .iter()
        .map(|block| match block {
            Block::Heading(inline) => div()
                .font_weight(FontWeight::BOLD)
                .child(styled(inline, cx))
                .into_any_element(),
            Block::Paragraph(inline) => div().child(styled(inline, cx)).into_any_element(),
            Block::Bullet(inline) => div()
                .flex()
                .gap_1()
                .child("•")
                .child(div().flex_1().min_w_0().child(styled(inline, cx)))
                .into_any_element(),
            Block::Code { language, text } => {
                let spans = super::code_highlight::highlight(language.as_deref(), text);
                div()
                    .px_1p5()
                    .py_1()
                    .rounded_sm()
                    .bg(colors.background)
                    .font_family(mono.clone())
                    .children(text.lines().enumerate().map(|(i, l)| {
                        let line = if l.is_empty() { " " } else { l };
                        let line_spans = spans.as_ref().and_then(|s| s.get(i));
                        div().child(super::code_highlight::styled_line(line, line_spans, cx))
                    }))
                    .into_any_element()
            }
            Block::Rule => div()
                .h(px(1.))
                .my_0p5()
                .bg(colors.border)
                .into_any_element(),
        })
        .collect()
}

/// An inline as a styled text element.
pub(crate) fn styled(inline: &Inline, cx: &App) -> StyledText {
    let colors = &theme(cx).colors;
    let highlights = inline.spans.iter().map(|(range, style)| {
        let h = match style {
            InlineStyle::Bold => HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            InlineStyle::Italic => HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            InlineStyle::Code => HighlightStyle {
                background_color: Some(colors.background),
                color: Some(colors.text),
                ..Default::default()
            },
            InlineStyle::Link => HighlightStyle {
                color: Some(colors.accent),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            },
        };
        (range.clone(), h)
    });
    StyledText::new(SharedString::from(inline.text.clone())).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline(text: &str, spans: &[(Range<usize>, InlineStyle)]) -> Inline {
        Inline {
            text: text.into(),
            spans: spans.to_vec(),
        }
    }

    #[test]
    fn blocks_and_inline_styles() {
        let md = "# Title\n\nUses `Vec` and **bold**,\n*it* and [docs](https://x.y).\n\n\
                  ```rust\nlet x = 1;\n\nx\n```\n- one\n- two_words\n---\nsnake_case_name\\*";
        let blocks = parse_markdown(md);
        assert_eq!(
            blocks,
            vec![
                Block::Heading(inline("Title", &[])),
                Block::Paragraph(inline(
                    "Uses Vec and bold, it and docs.",
                    &[
                        (5..8, InlineStyle::Code),
                        (13..17, InlineStyle::Bold),
                        (19..21, InlineStyle::Italic),
                        (26..30, InlineStyle::Link),
                    ]
                )),
                Block::Code {
                    language: Some("rust".into()),
                    text: "let x = 1;\n\nx".into(),
                },
                Block::Bullet(inline("one", &[])),
                Block::Bullet(inline("two_words", &[])),
                Block::Rule,
                Block::Paragraph(inline("snake_case_name*", &[])),
            ]
        );
    }

    #[test]
    fn unclosed_markers_stay_literal_and_breaks_are_kept() {
        let blocks = parse_markdown("a * b `c\nline two  \nthree\\\nfour");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(inline(
                "a * b `c line two\nthree\nfour",
                &[]
            ))]
        );
        assert_eq!(
            parse_plain("one\ntwo\n\n\nthree"),
            vec![
                Block::Paragraph(inline("one\ntwo", &[])),
                Block::Paragraph(inline("three", &[]))
            ]
        );
        assert_eq!(parse_markdown("####### x").len(), 1);
    }
}
