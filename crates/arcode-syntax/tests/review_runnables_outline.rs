//! Adversarial review tests for runnables and outline. Each test documents
//! one confirmed issue and fails against the current implementation.

mod common;

use std::time::Instant;

use arcode_syntax::RunnableKind;
use common::parse;

/// Python: `class TestFoo(unittest.TestCase)` matches both the `class Test*`
/// scope pattern and the `TestCase` scope pattern, so the class is pushed as
/// a scope twice and every method's `scope_path` repeats it.
#[test]
fn python_testcase_named_test_star_has_single_scope() {
    let src = "import unittest\n\nclass TestMath(unittest.TestCase):\n    def test_add(self):\n        pass\n";
    let (text, syntax) = parse("python", src);
    let items = syntax.runnables(&text);
    let test = items
        .iter()
        .find(|item| item.name == "test_add")
        .expect("test_add");
    assert_eq!(test.scope_path, ["TestMath"], "{items:#?}");
    assert_eq!(test.qualified_name("::"), "TestMath::test_add");
}

/// JS/TS test names collapse runs of whitespace inside the string literal
/// (`compact_text`), so the name no longer equals the test title. A
/// `jest -t` / `vitest -t` filter built from it does not match the test.
#[test]
fn javascript_test_name_keeps_inner_whitespace() {
    let src = "it(\"adds  two\\tnumbers\", () => {});\n";
    let (text, syntax) = parse("javascript", src);
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1, "{items:#?}");
    assert_eq!(items[0].name, "adds  two\\tnumbers");
}

/// `RunnableItem::line` and `OutlineItem::range` are tree-sitter rows (only
/// `\n` counts), while the editor and `highlight_lines` use ropey lines,
/// which also break at a lone `\r`, U+2028, U+2029, U+0085, VT and FF. A
/// U+2028 inside a JS string (legal since ES2019) shifts every later gutter
/// run button and outline entry by one line.
#[test]
fn runnable_and_outline_lines_match_rope_lines() {
    let src = "const s = 'a\u{2028}b';\nfunction f() {}\nit('x', () => {});\n";
    let (text, syntax) = parse("javascript", src);
    let run_byte = src.find("it(").expect("it");
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].line,
        text.byte_to_line(run_byte),
        "runnable line disagrees with the rope line"
    );
    let fn_byte = src.find("function").expect("function");
    let symbols = syntax.symbols(&text);
    let f = symbols.iter().find(|s| s.name == "f").expect("f");
    assert_eq!(f.range.start, text.byte_to_line(fn_byte));
}

/// `runnables` attaches scopes by testing every scope against every item,
/// which is quadratic: a JS spec file with many `describe` blocks takes
/// seconds (debug) instead of milliseconds.
#[test]
fn runnables_scope_assignment_is_not_quadratic() {
    let blocks = 8_000;
    let mut src = String::new();
    for i in 0..blocks {
        src.push_str(&format!("describe('s{i}', () => {{\n  it('t{i}', () => {{}});\n}});\n"));
    }
    let (text, syntax) = parse("javascript", &src);
    let small: String = src.lines().take(300).map(|l| format!("{l}\n")).collect();
    let (small_text, small_syntax) = parse("javascript", &small);

    let started = Instant::now();
    let small_items = small_syntax.runnables(&small_text);
    let small_time = started.elapsed();
    let started = Instant::now();
    let items = syntax.runnables(&text);
    let big_time = started.elapsed();
    assert_eq!(items.len(), blocks * 2);
    assert_eq!(small_items.len(), 200);
    let ratio = big_time.as_secs_f64() / small_time.as_secs_f64().max(1e-6);
    println!("small {small_time:?}, big {big_time:?}, ratio {ratio:.0} for 80x input");
    // 80x the input should cost about 80x, not 6400x.
    assert!(ratio < 400.0, "runnables grew {ratio:.0}x for 80x input");
    assert!(items[1].scope_path == ["s0"]);
    let _ = RunnableKind::Test;
}
