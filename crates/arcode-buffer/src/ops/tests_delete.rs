use super::test_util::{setup, show};
use super::*;

fn bs(marked: &str, cfg: &OpsConfig) -> String {
    let (mut b, mut s) = setup(marked);
    backspace(&mut b, &mut s, cfg);
    show(&b, &s)
}

#[test]
fn backspace_basics() {
    let c = OpsConfig::default();
    assert_eq!(bs("ab|c", &c), "a|c");
    assert_eq!(bs("|abc", &c), "|abc");
    assert_eq!(bs("a\n|b", &c), "a|b");
    assert_eq!(bs("a«bc»d", &c), "a|d");
    assert_eq!(bs("a|b|c|", &c), "|");
}

#[test]
fn backspace_unicode_graphemes() {
    let c = OpsConfig::default();
    assert_eq!(bs("x😀|", &c), "x|");
    assert_eq!(bs("e\u{301}|", &c), "|");
    assert_eq!(bs("中文|", &c), "中|");
    assert_eq!(bs("🇩🇪|x", &c), "|x");
}

#[test]
fn backspace_deletes_auto_pair() {
    let c = OpsConfig::default();
    assert_eq!(bs("(|)", &c), "|");
    assert_eq!(bs("\"|\"", &c), "|");
    assert_eq!(bs("(|x)", &c), "|x)");
    let off = OpsConfig {
        auto_close: AutoClose::disabled(),
        ..c
    };
    assert_eq!(bs("(|)", &off), "|)");
}

#[test]
fn backspace_to_indent_stop() {
    let c = OpsConfig::default();
    assert_eq!(bs("        |x", &c), "    |x");
    assert_eq!(bs("      |x", &c), "    |x");
    assert_eq!(bs("    |x", &c), "|x");
    assert_eq!(bs("  |", &c), "|");
    assert_eq!(bs("\t  |x", &c), "\t|x");
    // Not in leading whitespace: one char only.
    assert_eq!(bs("a       |x", &c), "a      |x");
    let two = OpsConfig {
        indent: IndentUnit::Spaces(2),
        ..c
    };
    assert_eq!(bs("     |x", &two), "    |x");
}

#[test]
fn backspace_overlapping_multi_cursor() {
    let c = OpsConfig::default();
    // Both cursors are in the same indentation; ranges overlap and merge.
    let (mut b, mut s) = setup("      |  |x");
    backspace(&mut b, &mut s, &c);
    assert_eq!(show(&b, &s), "    |x");
}

#[test]
fn delete_forward_cases() {
    let (mut b, mut s) = setup("a|bc|\nd|");
    delete_forward(&mut b, &mut s);
    assert_eq!(show(&b, &s), "a|c|d|");
    let (mut b, mut s) = setup("|😀x|e\u{301}");
    delete_forward(&mut b, &mut s);
    assert_eq!(show(&b, &s), "|x|");
    let (mut b, mut s) = setup("«ab»c");
    delete_forward(&mut b, &mut s);
    assert_eq!(show(&b, &s), "|c");
}

#[test]
fn delete_words() {
    let (mut b, mut s) = setup("foo.bar  baz|\nqux|");
    delete_word_left(&mut b, &mut s);
    assert_eq!(show(&b, &s), "foo.bar  |\n|");
    delete_word_left(&mut b, &mut s);
    assert_eq!(show(&b, &s), "foo.|");
    let (mut b, mut s) = setup("|foo bar|  baz");
    delete_word_right(&mut b, &mut s);
    assert_eq!(show(&b, &s), "| bar|");
}

#[test]
fn delete_undo_restores_selections() {
    let (mut b, mut s) = setup("«ab»c|d");
    delete_forward(&mut b, &mut s);
    assert_eq!(show(&b, &s), "|c|");
    let undo = b.undo().expect("undo");
    assert_eq!(show(&b, &undo.selections), "«ab»c|d");
    let redo = b.redo().expect("redo");
    assert_eq!(show(&b, &redo.selections), "|c|");
}
