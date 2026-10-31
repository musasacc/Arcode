//! Text objects: `iw aw iW aW`, quotes, brackets, tags, paragraphs.

mod pairs;

use std::ops::Range;

use arcode_buffer::Buffer;

use crate::text::{Class, ch, class, is_blank, last_line, line_blank, line_of};

/// What a text object selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjKind {
    Word(bool),
    Quote(char),
    Pair(char, char),
    Tag,
    Paragraph,
}

/// A text object: `kind` plus inner (`i`) or around (`a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextObject {
    pub kind: ObjKind,
    pub around: bool,
}

/// The selected range; `linewise` ranges cover whole lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjRange {
    pub range: Range<usize>,
    pub linewise: bool,
}

/// The object for the char after `i`/`a` (`w`, `"`, `(`, `b`, `t`, ...).
pub(crate) fn object_for(c: char, around: bool) -> Option<TextObject> {
    let kind = match c {
        'w' => ObjKind::Word(false),
        'W' => ObjKind::Word(true),
        '"' | '\'' | '`' => ObjKind::Quote(c),
        '(' | ')' | 'b' => ObjKind::Pair('(', ')'),
        '{' | '}' | 'B' => ObjKind::Pair('{', '}'),
        '[' | ']' => ObjKind::Pair('[', ']'),
        '<' | '>' => ObjKind::Pair('<', '>'),
        't' => ObjKind::Tag,
        'p' => ObjKind::Paragraph,
        _ => return None,
    };
    Some(TextObject { kind, around })
}

/// Selects `obj` around `pos`; `None` if there is no such object.
pub(crate) fn select(b: &Buffer, pos: usize, obj: TextObject, count: usize) -> Option<ObjRange> {
    let count = count.max(1);
    match obj.kind {
        ObjKind::Word(big) => word(b, pos, big, obj.around, count),
        ObjKind::Quote(q) => quote(b, pos, q, obj.around),
        ObjKind::Pair(o, c) => pairs::pair(b, pos, o, c, obj.around, count),
        ObjKind::Tag => pairs::tag(b, pos, obj.around, count),
        ObjKind::Paragraph => paragraph(b, pos, obj.around, count),
    }
}

fn charwise(range: Range<usize>) -> Option<ObjRange> {
    Some(ObjRange {
        range,
        linewise: false,
    })
}

fn word(b: &Buffer, pos: usize, big: bool, around: bool, count: usize) -> Option<ObjRange> {
    let line = line_of(b, pos);
    let (ls, le) = (b.line_start(line), b.line_end(line));
    if le == ls {
        return None;
    }
    let pos = pos.min(le - 1);
    let cls = |i: usize| class(ch(b, i), big);
    let run_start = |i: usize| {
        let c = cls(i);
        let mut s = i;
        while s > ls && cls(s - 1) == c {
            s -= 1;
        }
        s
    };
    let run_end = |i: usize| {
        let c = cls(i);
        let mut e = i;
        while e < le && cls(e) == c {
            e += 1;
        }
        e
    };
    let (mut start, mut end) = (run_start(pos), run_end(pos));
    if !around {
        for _ in 1..count {
            if end < le {
                end = run_end(end);
            }
        }
        return charwise(start..end);
    }
    if cls(pos) == Class::Space {
        if end < le {
            end = run_end(end);
        }
    } else if end < le && cls(end) == Class::Space {
        end = run_end(end);
    } else if start > ls && cls(start - 1) == Class::Space {
        start = run_start(start - 1);
    }
    for _ in 1..count {
        for _ in 0..2 {
            if end < le {
                end = run_end(end);
            }
        }
    }
    charwise(start..end)
}

fn quote(b: &Buffer, pos: usize, q: char, around: bool) -> Option<ObjRange> {
    let line = line_of(b, pos);
    let (ls, le) = (b.line_start(line), b.line_end(line));
    let quotes: Vec<usize> = (ls..le)
        .filter(|&i| ch(b, i) == q && (i == ls || ch(b, i - 1) != '\\'))
        .collect();
    let (open, close) = if let Some(idx) = quotes.iter().position(|&p| p == pos) {
        if idx % 2 == 0 {
            (quotes[idx], *quotes.get(idx + 1)?)
        } else {
            (quotes[idx - 1], quotes[idx])
        }
    } else {
        let before = quotes.iter().rev().find(|&&p| p < pos).copied();
        let mut after = quotes.iter().filter(|&&p| p > pos).copied();
        match before {
            Some(open) => (open, after.next()?),
            None => (after.next()?, after.next()?),
        }
    };
    if !around {
        return charwise(open + 1..close);
    }
    let (mut start, mut end) = (open, close + 1);
    if end < le && is_blank(ch(b, end)) {
        while end < le && is_blank(ch(b, end)) {
            end += 1;
        }
    } else {
        while start > ls && is_blank(ch(b, start - 1)) {
            start -= 1;
        }
    }
    charwise(start..end)
}

fn paragraph(b: &Buffer, pos: usize, around: bool, count: usize) -> Option<ObjRange> {
    let last = last_line(b);
    let line = line_of(b, pos);
    let kind = line_blank(b, line);
    let (mut first, mut end) = (line, line);
    while first > 0 && line_blank(b, first - 1) == kind {
        first -= 1;
    }
    while end < last && line_blank(b, end + 1) == kind {
        end += 1;
    }
    let runs = if around { 2 * count } else { count };
    for _ in 1..runs {
        if end >= last {
            break;
        }
        end += 1;
        let k = line_blank(b, end);
        while end < last && line_blank(b, end + 1) == k {
            end += 1;
        }
    }
    if around && !kind && !line_blank(b, end) {
        while first > 0 && line_blank(b, first - 1) {
            first -= 1;
        }
    }
    let stop = if end < last {
        b.line_start(end + 1)
    } else {
        b.len_chars()
    };
    Some(ObjRange {
        range: b.line_start(first)..stop,
        linewise: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(text: &str, pos: usize, c: char, around: bool) -> Option<String> {
        let b = Buffer::new(text);
        let obj = object_for(c, around)?;
        select(&b, pos, obj, 1).map(|r| b.slice_to_string(r.range))
    }

    #[test]
    fn words_and_quotes() {
        assert_eq!(sel("foo bar baz", 5, 'w', false).as_deref(), Some("bar"));
        assert_eq!(sel("foo bar baz", 5, 'w', true).as_deref(), Some("bar "));
        assert_eq!(sel("foo bar", 5, 'w', true).as_deref(), Some(" bar"));
        assert_eq!(sel("a.b c", 0, 'W', false).as_deref(), Some("a.b"));
        assert_eq!(sel("x \"ab\" y", 3, '"', false).as_deref(), Some("ab"));
        assert_eq!(sel("x \"ab\" y", 3, '"', true).as_deref(), Some("\"ab\" "));
        assert_eq!(sel("x \"ab\"", 0, '"', false).as_deref(), Some("ab"));
        assert_eq!(sel("'a' 'b'", 4, '\'', false).as_deref(), Some("b"));
        assert_eq!(sel("no quotes", 0, '"', false), None);
    }

    #[test]
    fn paragraphs() {
        let text = "a\nb\n\n\nc\n";
        assert_eq!(sel(text, 0, 'p', false).as_deref(), Some("a\nb\n"));
        assert_eq!(sel(text, 0, 'p', true).as_deref(), Some("a\nb\n\n\n"));
        assert_eq!(sel(text, 4, 'p', false).as_deref(), Some("\n\n"));
        assert_eq!(sel(text, 4, 'p', true).as_deref(), Some("\n\nc\n"));
        assert_eq!(sel("x\n\ny", 3, 'p', true).as_deref(), Some("\ny"));
    }
}
