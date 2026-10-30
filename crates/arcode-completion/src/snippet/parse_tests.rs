use super::*;
use crate::snippet::ast::{CaseOp, FormatItem};

fn p(s: &str) -> Vec<Node> {
    parse(s).nodes
}

fn text(s: &str) -> Node {
    Node::Text(s.into())
}

fn tab(index: u32) -> Node {
    Node::Tabstop {
        index,
        transform: None,
    }
}

#[test]
fn tabstops_and_text() {
    assert_eq!(
        p("a $1 b ${2} $0"),
        [text("a "), tab(1), text(" b "), tab(2), text(" "), tab(0)]
    );
    assert_eq!(p("$12x"), [tab(12), text("x")]);
}

#[test]
fn nested_placeholders() {
    assert_eq!(
        p("${1:foo ${2:bar $3}}"),
        [Node::Placeholder {
            index: 1,
            children: vec![
                text("foo "),
                Node::Placeholder {
                    index: 2,
                    children: vec![text("bar "), tab(3)],
                },
            ],
        }]
    );
    assert_eq!(
        p("${1:}"),
        [Node::Placeholder {
            index: 1,
            children: vec![],
        }]
    );
}

#[test]
fn choices() {
    assert_eq!(
        p("${1|one,two,three|}"),
        [Node::Choice {
            index: 1,
            options: vec!["one".into(), "two".into(), "three".into()],
        }]
    );
    assert_eq!(
        p(r"${1|a\,b,c\|d|}"),
        [Node::Choice {
            index: 1,
            options: vec!["a,b".into(), "c|d".into()],
        }]
    );
    // Unterminated choice stays text.
    assert_eq!(p("${1|a,b}"), [text("${1|a,b}")]);
}

#[test]
fn escapes() {
    assert_eq!(p(r"\$1 \} \\ \n"), [text(r"$1 } \ \n")]);
    assert_eq!(
        p(r"${1:a\}b}"),
        [Node::Placeholder {
            index: 1,
            children: vec![text("a}b")],
        }]
    );
    // A lone `}` at top level is text.
    assert_eq!(p("}"), [text("}")]);
}

#[test]
fn variables() {
    assert_eq!(
        p("$TM_FILENAME"),
        [Node::Variable {
            name: "TM_FILENAME".into(),
            default: None,
            transform: None,
        }]
    );
    assert_eq!(
        p("${FOO:bar $1}"),
        [Node::Variable {
            name: "FOO".into(),
            default: Some(vec![text("bar "), tab(1)]),
            transform: None,
        }]
    );
    match &p("${TM_FILENAME/(.*)\\..+$/$1/}")[0] {
        Node::Variable {
            transform: Some(t), ..
        } => {
            assert_eq!(t.regex, "(.*)\\..+$");
            assert_eq!(t.format, [FormatItem::Group(1)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn transforms() {
    match &p("${1/(a)|(b)/${1:+A}${2:?B:N}${1:-x}${1:y}\\/${1:/capitalize}/gi}")[0] {
        Node::Tabstop {
            index: 1,
            transform: Some(t),
        } => {
            assert_eq!(t.regex, "(a)|(b)");
            assert_eq!(t.flags, "gi");
            assert_eq!(t.format.len(), 6);
            assert_eq!(t.format[4], FormatItem::Text("/".into()));
            assert_eq!(t.format[5], FormatItem::Case(1, CaseOp::Capitalize));
            assert_eq!(
                t.format[1],
                FormatItem::Conditional {
                    group: 2,
                    if_text: "B".into(),
                    else_text: "N".into(),
                }
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn invalid_constructs_are_text() {
    assert_eq!(p("$"), [text("$")]);
    assert_eq!(p("${"), [text("${")]);
    assert_eq!(p("${1:abc"), [text("${1:abc")]);
    assert_eq!(p("${1x}"), [text("${1x}")]);
    assert_eq!(p("price: $ 5"), [text("price: $ 5")]);
    assert_eq!(
        p("${1:a ${2:b}"),
        [
            text("${1:a "),
            Node::Placeholder {
                index: 2,
                children: vec![text("b")],
            }
        ]
    );
    assert_eq!(p("$99999999999"), [text("$99999999999")]);
}

#[test]
fn unicode_and_crlf() {
    assert_eq!(
        p("ä${1:ö}ü\r\n"),
        [
            text("ä"),
            Node::Placeholder {
                index: 1,
                children: vec![text("ö")],
            },
            text("ü\n")
        ]
    );
}

#[test]
fn deep_unclosed_nesting_is_fast() {
    let src = "${1:".repeat(2000);
    let start = std::time::Instant::now();
    let nodes = p(&src);
    assert!(start.elapsed().as_secs() < 5);
    assert_eq!(nodes, [text(&src)]);
}

#[test]
fn preview_text() {
    let s = Snippet::parse("fn ${1:name}(${2|a,b|}) $VAR ${X:d}$0");
    assert_eq!(s.preview_text(), "fn name(a) VAR d");
}
