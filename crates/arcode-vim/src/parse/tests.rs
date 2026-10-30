//! Parser tests.

use super::*;
use crate::key::Key;
use crate::motion::Motion;

fn p(keys: &str, visual: bool) -> Parse {
    let toks: Vec<Tok> = Key::parse_keys(keys).iter().map(Key::tok).collect();
    parse(
        &toks,
        ParseCtx {
            visual,
            recording: false,
        },
    )
}

fn done(keys: &str) -> (Option<usize>, Option<char>, CmdKind) {
    match p(keys, false) {
        Parse::Done(c) => (c.count, c.register, c.kind),
        other => panic!("{keys}: {other:?}"),
    }
}

#[test]
fn counts_and_registers() {
    let dw = CmdKind::Operate(Op::Delete, Target::Motion(Motion::WordStart(false)));
    assert_eq!(done("dw"), (None, None, dw));
    assert_eq!(done("2d3w"), (Some(6), None, dw));
    assert_eq!(done("\"a2dw"), (Some(2), Some('a'), dw));
    assert_eq!(done("10j").0, Some(10));
    assert_eq!(done("0").2, CmdKind::Move(Motion::LineStart));
    assert_eq!(done("dd").2, CmdKind::Operate(Op::Delete, Target::Line));
    assert_eq!(done("gUU").2, CmdKind::Operate(Op::Upper, Target::Line));
    assert_eq!(done("gUgU").2, CmdKind::Operate(Op::Upper, Target::Line));
    assert_eq!(done("gcc").2, CmdKind::Operate(Op::Comment, Target::Line));
    assert!(matches!(
        done("gUgg").2,
        CmdKind::Operate(Op::Upper, Target::Motion(Motion::FileStart))
    ));
}

#[test]
fn incomplete_and_invalid() {
    assert_eq!(p("d", false), MORE_OP);
    assert_eq!(p("d2", false), MORE_OP);
    assert_eq!(p("di", false), MORE_OP);
    assert_eq!(p("f", false), MORE);
    assert_eq!(p("\"", false), MORE);
    assert_eq!(p("g", false), MORE);
    assert_eq!(p("dQ", false), Parse::Invalid);
    assert_eq!(p("\"!", false), Parse::Invalid);
    assert_eq!(p("Q", false), Parse::Invalid);
    assert!(matches!(
        p("<c-s>", false),
        Parse::Done(Cmd {
            kind: CmdKind::Act(Act::PassThrough),
            ..
        })
    ));
    assert_eq!(p("d<c-s>", false), Parse::Invalid);
}

#[test]
fn visual() {
    let k = |keys| match p(keys, true) {
        Parse::Done(c) => c.kind,
        other => panic!("{other:?}"),
    };
    assert_eq!(k("d"), CmdKind::VisualOp(Op::Delete, false));
    assert_eq!(k("X"), CmdKind::VisualOp(Op::Delete, true));
    assert!(matches!(k("iw"), CmdKind::Object(_)));
    assert_eq!(k("gU"), CmdKind::VisualOp(Op::Upper, false));
    assert_eq!(k("I"), CmdKind::Act(Act::BlockInsert(false)));
}
