use super::test_util::{setup, show};
use super::*;

fn run(marked: &str, f: impl Fn(&mut crate::Buffer, &mut crate::SelectionSet)) -> String {
    let (mut b, mut s) = setup(marked);
    f(&mut b, &mut s);
    show(&b, &s)
}

#[test]
fn indent_and_outdent() {
    let sp = IndentUnit::Spaces(4);
    assert_eq!(
        run("a|b\nc", |b, s| drop(indent_lines(b, s, sp))),
        "    a|b\nc"
    );
    assert_eq!(
        run("«a\n\nb»\nc", |b, s| drop(indent_lines(b, s, sp))),
        "«    a\n\n    b»\nc"
    );
    // A selection ending at column 0 excludes that line.
    assert_eq!(
        run("«a\n»b", |b, s| drop(indent_lines(b, s, IndentUnit::Tab))),
        "«\ta\n»b"
    );
    assert_eq!(
        run("      a|\n\tb|\n  c|\nd|", |b, s| drop(outdent_lines(
            b, s, sp, 4
        ))),
        "  a|\nb|\nc|\nd|"
    );
    assert_eq!(
        run("\t\tx|", |b, s| drop(outdent_lines(
            b,
            s,
            IndentUnit::Tab,
            4
        ))),
        "\tx|"
    );
    assert_eq!(
        run("        x|", |b, s| drop(outdent_lines(
            b,
            s,
            IndentUnit::Tab,
            2
        ))),
        "      x|"
    );
}

#[test]
fn indent_multi_cursor_same_line_once() {
    let out = run("a|b|c", |b, s| {
        drop(indent_lines(b, s, IndentUnit::Spaces(2)))
    });
    assert_eq!(out, "  a|b|c");
}

#[test]
fn duplicate_lines() {
    assert_eq!(
        run("ab|c\nx", |b, s| drop(duplicate_line(b, s))),
        "abc\nab|c\nx"
    );
    assert_eq!(run("a\nb|", |b, s| drop(duplicate_line(b, s))), "a\nb\nb|");
    assert_eq!(
        run("«a\nb»\nc", |b, s| drop(duplicate_line(b, s))),
        "a\nb\n«a\nb»\nc"
    );
    assert_eq!(
        run("a|\nb|\nc", |b, s| drop(duplicate_line(b, s))),
        "a\nb\na|\nb|\nc"
    );
    let (mut b, mut s) = setup("x|\ny");
    duplicate_line(&mut b, &mut s);
    b.undo();
    assert_eq!(b.to_string(), "x\ny");
}

#[test]
fn duplicate_lines_up() {
    assert_eq!(
        run("ab|c\nx", |b, s| drop(duplicate_line_up(b, s))),
        "ab|c\nabc\nx"
    );
    assert_eq!(
        run("a\nb|", |b, s| drop(duplicate_line_up(b, s))),
        "a\nb|\nb"
    );
    assert_eq!(
        run("«a\nb»\nc", |b, s| drop(duplicate_line_up(b, s))),
        "«a\nb»\na\nb\nc"
    );
    // Each block's selections stay on its upper copy, after the copies of
    // the blocks above.
    assert_eq!(
        run("a|\nx\nb|\nc", |b, s| drop(duplicate_line_up(b, s))),
        "a|\na\nx\nb|\nb\nc"
    );
    let (mut b, mut s) = setup("x|\ny");
    duplicate_line_up(&mut b, &mut s);
    b.undo();
    assert_eq!(b.to_string(), "x\ny");
}

#[test]
fn move_lines_up_down() {
    assert_eq!(
        run("a\nb|\nc", |b, s| drop(move_lines_up(b, s))),
        "b|\na\nc"
    );
    assert_eq!(
        run("a|\nb\nc", |b, s| drop(move_lines_up(b, s))),
        "a|\nb\nc"
    );
    assert_eq!(
        run("a\nb|\nc", |b, s| drop(move_lines_down(b, s))),
        "a\nc\nb|"
    );
    assert_eq!(
        run("a\nb\nc|", |b, s| drop(move_lines_down(b, s))),
        "a\nb\nc|"
    );
    assert_eq!(
        run("1\n«2\n3»\n4", |b, s| drop(move_lines_up(b, s))),
        "«2\n3»\n1\n4"
    );
    assert_eq!(
        run("1\n«2\n»3\n4", |b, s| drop(move_lines_down(b, s))),
        "1\n3\n«2\n»4"
    );
    // Two separate blocks move together.
    assert_eq!(
        run("a\nb|\nc\nd|", |b, s| drop(move_lines_up(b, s))),
        "b|\na\nd|\nc"
    );
    // Unicode content.
    assert_eq!(
        run("😀\n中|文", |b, s| drop(move_lines_up(b, s))),
        "中|文\n😀"
    );
}

#[test]
fn move_lines_round_trip_and_undo() {
    let (mut b, mut s) = setup("one\ntw|o\nthree");
    move_lines_down(&mut b, &mut s);
    move_lines_up(&mut b, &mut s);
    assert_eq!(show(&b, &s), "one\ntw|o\nthree");
    b.undo();
    assert_eq!(b.to_string(), "one\nthree\ntwo");
    let undo = b.undo().expect("undo");
    assert_eq!(show(&b, &undo.selections), "one\ntw|o\nthree");
}

#[test]
fn toggle_comment() {
    let t = |m: &str| run(m, |b, s| drop(toggle_line_comment(b, s, "//")));
    assert_eq!(t("  fo|o"), "  // fo|o");
    assert_eq!(t("  // fo|o"), "  fo|o");
    assert_eq!(t("  //fo|o"), "  fo|o");
    assert_eq!(t("«    a\n\n  b»"), "«  //   a\n\n  // b»");
    assert_eq!(t("«  //   a\n\n  // b»"), "«    a\n\n  b»");
    // Mixed: some commented -> comment all.
    assert_eq!(t("«// a\nb»"), "«// // a\n// b»");
    assert_eq!(t("|"), "// |");
    assert_eq!(t("x|"), "// x|");
    assert_eq!(
        run("a|", |b, s| drop(toggle_line_comment(b, s, "  "))),
        "a|"
    );
    assert_eq!(
        run("a|", |b, s| drop(toggle_line_comment(b, s, "#"))),
        "# a|"
    );
}

#[test]
fn toggle_comment_multi_cursor_undo() {
    let (mut b, mut s) = setup("a|\nb\nc|");
    toggle_line_comment(&mut b, &mut s, "#");
    assert_eq!(show(&b, &s), "# a|\nb\n# c|");
    let undo = b.undo().expect("undo");
    assert_eq!(show(&b, &undo.selections), "a|\nb\nc|");
    b.redo();
    assert_eq!(b.to_string(), "# a\nb\n# c");
}
