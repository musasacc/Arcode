//! Bracket and tag text objects.

use arcode_buffer::Buffer;

use super::{ObjRange, charwise};
use crate::motion::find::{find_close, find_open};
use crate::text::{ch, is_blank, line_of};

/// `i(`, `a(` and friends; `count` selects enclosing levels outwards.
///
/// An inner block whose `(` ends its line and whose `)` starts (after
/// blanks) its line is linewise: only the lines in between.
pub(super) fn pair(
    b: &Buffer,
    pos: usize,
    open: char,
    close: char,
    around: bool,
    count: usize,
) -> Option<ObjRange> {
    // On a closing bracket, `find_open` from `pos` finds its partner.
    let mut o = if ch(b, pos) == open {
        pos
    } else {
        find_open(b, pos, open, close)?
    };
    let mut cl = find_close(b, o + 1, open, close)?;
    for _ in 1..count {
        o = find_open(b, o, open, close)?;
        cl = find_close(b, o + 1, open, close)?;
    }
    if around {
        return charwise(o..cl + 1);
    }
    let mut start = o + 1;
    let mut end = cl;
    if ch(b, start) == '\n' && start < end {
        start += 1;
    }
    let close_line = line_of(b, cl);
    let close_line_start = b.line_start(close_line);
    let close_first = close_line > line_of(b, o)
        && b.slice_to_string(close_line_start..cl)
            .chars()
            .all(is_blank);
    if close_first {
        end = close_line_start.max(start);
    }
    let start = start.min(end);
    Some(ObjRange {
        range: start..end,
        linewise: close_first && start < end && start == b.line_start(line_of(b, start)),
    })
}

/// One parsed tag.
struct Tag {
    start: usize,
    end: usize,
    name: String,
    closing: bool,
}

/// Scans `text` (chars) for `<name ...>` and `</name>` tags, skipping
/// self-closing tags, comments and declarations.
fn scan_tags(text: &[char]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] != '<' {
            i += 1;
            continue;
        }
        let closing = text.get(i + 1) == Some(&'/');
        let name_start = i + 1 + usize::from(closing);
        let mut j = name_start;
        while j < text.len()
            && (text[j].is_alphanumeric() || matches!(text[j], '-' | '_' | ':' | '.'))
        {
            j += 1;
        }
        if j == name_start {
            i += 1;
            continue;
        }
        let Some(gt) = text[j..].iter().position(|&c| c == '>' || c == '<') else {
            break;
        };
        let gt = j + gt;
        if text[gt] == '<' {
            i = gt;
            continue;
        }
        if !(text[gt - 1] == '/' && !closing) {
            tags.push(Tag {
                start: i,
                end: gt + 1,
                name: text[name_start..j].iter().collect(),
                closing,
            });
        }
        i = gt + 1;
    }
    tags
}

/// `it` / `at`: the `count`-th innermost tag pair containing `pos`.
pub(super) fn tag(b: &Buffer, pos: usize, around: bool, count: usize) -> Option<ObjRange> {
    let text: Vec<char> = b.text().chars().collect();
    let tags = scan_tags(&text);
    let mut stack: Vec<&Tag> = Vec::new();
    let mut pairs: Vec<(&Tag, &Tag)> = Vec::new();
    for t in &tags {
        if !t.closing {
            stack.push(t);
        } else if let Some(idx) = stack.iter().rposition(|o| o.name == t.name) {
            pairs.push((stack[idx], t));
            stack.truncate(idx);
        }
    }
    let mut containing: Vec<(&Tag, &Tag)> = pairs
        .into_iter()
        .filter(|(o, c)| o.start <= pos && pos < c.end)
        .collect();
    containing.sort_by_key(|(o, c)| c.end - o.start);
    let (o, c) = containing.get(count - 1)?;
    if around {
        charwise(o.start..c.end)
    } else {
        charwise(o.end..c.start)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{object_for, select};
    use arcode_buffer::Buffer;

    fn sel(text: &str, pos: usize, obj: &str, count: usize) -> Option<(String, bool)> {
        let b = Buffer::new(text);
        let mut cs = obj.chars();
        let around = cs.next() == Some('a');
        let o = object_for(cs.next()?, around)?;
        select(&b, pos, o, count).map(|r| (b.slice_to_string(r.range), r.linewise))
    }

    #[test]
    fn brackets() {
        let s = |t, p, o, n| sel(t, p, o, n).map(|x| x.0);
        assert_eq!(s("f(a, (b))", 6, "i(", 1).as_deref(), Some("b"));
        assert_eq!(s("f(a, (b))", 6, "i(", 2).as_deref(), Some("a, (b)"));
        assert_eq!(s("f(a, (b))", 6, "ab", 2).as_deref(), Some("(a, (b))"));
        assert_eq!(s("f(a, (b))", 1, "i)", 1).as_deref(), Some("a, (b)"));
        assert_eq!(s("f(a, (b))", 8, "i)", 1).as_deref(), Some("a, (b)"));
        assert_eq!(s("x[1]", 2, "a]", 1).as_deref(), Some("[1]"));
        assert_eq!(s("x <a> y", 3, "i<", 1).as_deref(), Some("a"));
        assert_eq!(s("none", 1, "i(", 1), None);
        assert_eq!(s("()", 0, "i(", 1).as_deref(), Some(""));
        assert_eq!(
            sel("f {\n  x;\n}", 2, "iB", 1),
            Some(("  x;\n".to_string(), true))
        );
        assert_eq!(sel("{\n}", 0, "i{", 1), Some((String::new(), false)));
    }

    #[test]
    fn tags() {
        let t = "<div><p a=\"1\">hi</p><br/></div>";
        let s = |p, o, n| sel(t, p, o, n).map(|x| x.0);
        assert_eq!(s(15, "it", 1).as_deref(), Some("hi"));
        assert_eq!(s(15, "at", 1).as_deref(), Some("<p a=\"1\">hi</p>"));
        assert_eq!(s(15, "it", 2).as_deref(), Some("<p a=\"1\">hi</p><br/>"));
        assert_eq!(s(21, "it", 1).as_deref(), Some("<p a=\"1\">hi</p><br/>"));
        assert_eq!(s(15, "it", 3), None);
    }
}
