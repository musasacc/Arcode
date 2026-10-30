//! Adversarial review tests for runnables and outline. Each test documents
//! one confirmed issue and fails against the current implementation.

mod common;

use std::time::{Duration, Instant};

use arcode_syntax::{OutlineKind, Syntax};
use common::parse;
use ropey::Rope;

/// Best of three runs of `f`.
fn best_of_three(mut f: impl FnMut()) -> Duration {
    (0..3)
        .map(|_| {
            let started = Instant::now();
            f();
            started.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// The Rust runnables query starts a match state at every `attribute_item`
/// and keeps it alive across the following `[(attribute_item) (line_comment)
/// (block_comment)]*` siblings. A run of n attributes keeps ~3n states alive
/// and tree-sitter compares their captures pairwise, so the cost is cubic in
/// the run length (debug: 100 attributes 1.1 s, 200 attributes 8.5 s, 400
/// attributes 67 s). Real code hits this: bindgen output (`#[doc = "..."]`
/// per doc line) or rstest `#[case(..)]` stacks. Here a 2,000-line
/// bindgen-style file takes ~1.7 s in `runnables` against ~10 ms in
/// `symbols` for the same tree.
#[test]
fn rust_runnables_cost_is_not_cubic_in_attribute_runs() {
    let mut src = String::new();
    for i in 0..50 {
        for j in 0..40 {
            src.push_str(&format!("#[doc = \" line {j}\"]\n"));
        }
        src.push_str(&format!("pub struct S{i} {{ pub x: u32 }}\n"));
    }
    let (text, syntax) = parse("rust", &src);
    // Compile both queries before timing.
    let _ = syntax.runnables(&Rope::new());
    let _ = syntax.symbols(&Rope::new());

    let symbols = best_of_three(|| {
        assert_eq!(syntax.symbols(&text).len(), 50);
    });
    let runnables = best_of_three(|| {
        assert!(syntax.runnables(&text).is_empty());
    });
    println!("2k-line bindgen-style file: symbols {symbols:?}, runnables {runnables:?}");
    assert!(
        runnables < symbols * 20 + Duration::from_millis(20),
        "runnables took {runnables:?} vs symbols {symbols:?}"
    );
}

/// Python: `class TestFoo(unittest.TestCase)` matches both the `class Test*`
/// scope pattern and the `...TestCase` scope pattern, so the class is pushed
/// as a scope twice and every method's `scope_path` repeats it (a test id
/// built from it, `TestMath::TestMath::test_add`, does not exist).
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

/// JS/TS test names go through `compact_text`, which collapses runs of
/// whitespace inside the string literal, so the name no longer equals the
/// test title and a `jest -t` / `vitest -t` filter built from it misses.
#[test]
fn javascript_test_name_keeps_inner_whitespace() {
    let src = "it(\"adds  two\", () => {});\n";
    let (text, syntax) = parse("javascript", src);
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1, "{items:#?}");
    assert_eq!(items[0].name, "adds  two");
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

/// `runnables` assigns scopes by testing every scope against every item
/// (O(items x scopes)). Debug timings: 2,000 `describe` blocks 0.1 s,
/// 16,000 blocks 4.3 s; 8x the input costs ~40x.
#[test]
fn runnables_scope_assignment_is_not_quadratic() {
    fn spec(blocks: usize) -> (Rope, Syntax) {
        let mut src = String::new();
        for i in 0..blocks {
            src.push_str(&format!(
                "describe('s{i}', () => {{\n  it('t{i}', () => {{}});\n}});\n"
            ));
        }
        parse("javascript", &src)
    }
    let (small_text, small) = spec(2_000);
    let (big_text, big) = spec(16_000);
    let _ = small.runnables(&Rope::new());
    let small_time = best_of_three(|| assert_eq!(small.runnables(&small_text).len(), 4_000));
    let big_time = best_of_three(|| assert_eq!(big.runnables(&big_text).len(), 32_000));
    let ratio = big_time.as_secs_f64() / small_time.as_secs_f64().max(1e-6);
    println!("2k blocks {small_time:?}, 16k blocks {big_time:?}, ratio {ratio:.1}");
    assert!(ratio < 20.0, "8x the input cost {ratio:.1}x");
}

/// Go: `const a, b = 1, 2` produces one match per name with the same
/// `@item` (the `const_spec`), and the "same item: later pattern wins"
/// dedup keeps only one of them, so `b` (and `Y` in a const block) is
/// missing from the outline.
#[test]
fn go_outline_lists_every_name_of_a_multi_name_const() {
    let src = "package p\nconst a, b = 1, 2\nconst (\n\tX, Y = 1, 2\n)\n";
    let (text, syntax) = parse("go", src);
    let names: Vec<String> = syntax
        .symbols(&text)
        .into_iter()
        .filter(|item| item.kind == OutlineKind::Const)
        .map(|item| item.name)
        .collect();
    assert_eq!(names, ["a", "b", "X", "Y"]);
}

/// Markdown: the heading name of `## Sub ##` keeps the optional closing
/// sequence (`"Sub ##"`), so the outline/breadcrumb shows stray hashes.
#[test]
fn markdown_heading_name_drops_closing_hashes() {
    let src = "# Title\n\n## Sub ##\n\ntext\n";
    let (text, syntax) = parse("markdown", src);
    let names: Vec<String> = syntax
        .symbols(&text)
        .into_iter()
        .map(|item| item.name)
        .collect();
    assert_eq!(names, ["Title", "Sub"]);
}

/// C: functions returning a pointer to pointer (`char **dup(void)`) have a
/// nested `pointer_declarator`, which the outline query does not match, so
/// they are missing from the outline.
#[test]
fn c_outline_includes_double_pointer_functions() {
    let src = "char **dup(void) { return 0; }\nint plain(void) { return 0; }\n";
    let (text, syntax) = parse("c", src);
    let names: Vec<String> = syntax
        .symbols(&text)
        .into_iter()
        .map(|item| item.name)
        .collect();
    assert_eq!(names, ["dup", "plain"]);
}

/// C: `int main` is only found as a direct child of `translation_unit`, so
/// a `main` inside a preprocessor conditional (`#ifdef STANDALONE ...
/// #endif`, common for self-testing C files) gets no run button.
#[test]
fn c_main_inside_preprocessor_conditional_is_runnable() {
    let src = "#ifdef STANDALONE\nint main(void) { return 0; }\n#endif\n";
    let (text, syntax) = parse("c", src);
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1, "{items:#?}");
    assert_eq!(items[0].line, 1);
    assert_eq!(items[0].name, "main");
}
