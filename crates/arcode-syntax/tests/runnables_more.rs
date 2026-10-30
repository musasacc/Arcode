//! Runnables edge cases: attribute runs, framework variants, rope lines.

mod common;

use std::time::Instant;

use arcode_syntax::OutlineKind::{Function, Method, Module};
use arcode_syntax::RunnableKind::{self, Main, Test, TestModule};
use common::{o, outline, parse};

/// `(line, kind, qualified name)` for every runnable in `src`.
fn runnables(id: &str, src: &str) -> Vec<(usize, RunnableKind, String)> {
    let (text, syntax) = parse(id, src);
    syntax
        .runnables(&text)
        .into_iter()
        .map(|item| (item.line, item.kind, item.qualified_name("::")))
        .collect()
}

fn r(line: usize, kind: RunnableKind, name: &str) -> (usize, RunnableKind, String) {
    (line, kind, name.to_string())
}

#[test]
fn rust_attribute_runs_with_comments_and_docs() {
    let src = "/// Doc.\n#[test]\n// note\n#[should_panic]\n/* block */\n#[ignore]\nfn a() {}\n\n#[test]\nstruct NotAFn;\nfn b() {}\n\n#[tokio::test]\nasync fn c() {}\n";
    assert_eq!(
        runnables("rust", src),
        vec![r(6, Test, "a"), r(13, Test, "c")]
    );
}

#[test]
fn rust_cfg_all_test_is_a_test_module() {
    let src = "#[cfg(all(test, unix))]\nmod unix_tests {\n    #[test]\n    fn t() {}\n}\n#[cfg(not(test))]\nmod real {}\n#[cfg(test)]\nmod external;\n";
    assert_eq!(
        runnables("rust", src),
        vec![r(1, TestModule, "unix_tests"), r(3, Test, "unix_tests::t")]
    );
}

#[test]
fn rust_many_test_attributes_stay_fast() {
    let mut src = String::new();
    for i in 0..3_000 {
        src.push_str(&format!("#[test]\n#[doc = \"x\"]\nfn t{i}() {{}}\n"));
    }
    src.push_str(&"#[test]\n".repeat(2_000));
    src.push_str("fn last() {}\n");
    let (text, syntax) = parse("rust", &src);
    let started = Instant::now();
    let items = syntax.runnables(&text);
    let elapsed = started.elapsed();
    assert_eq!(items.len(), 3_001);
    assert_eq!(items[3_000].name, "last");
    assert_eq!(items[3_000].line, 9_000 + 2_000);
    assert!(elapsed.as_secs() < 5, "took {elapsed:?}");
}

#[test]
fn python_reversed_main_check() {
    let src = "def run():\n    pass\n\nif '__main__' == __name__:\n    run()\n";
    assert_eq!(runnables("python", src), vec![r(3, Main, "__main__")]);
}

#[test]
fn python_testcase_scope_is_not_repeated() {
    let src = "import unittest\n\nclass TestMath(unittest.TestCase):\n    def test_add(self):\n        pass\n\n    def helper(self):\n        pass\n";
    assert_eq!(
        runnables("python", src),
        vec![
            r(2, TestModule, "TestMath"),
            r(3, Test, "TestMath::test_add")
        ]
    );
}

#[test]
fn playwright_describe_and_steps() {
    let src = "test.describe('suite', () => {\n  test('a', async () => {\n    await test.step('s', async () => {});\n  });\n});\ntest.describe.serial('serial', () => {\n  test.only('b', () => {});\n});\n";
    assert_eq!(
        runnables("typescript", src),
        vec![
            r(0, TestModule, "suite"),
            r(1, Test, "suite::a"),
            r(5, TestModule, "serial"),
            r(6, Test, "serial::b"),
        ]
    );
}

#[test]
fn jest_each_tables() {
    let src = "describe.each([[1]])('math %i', (n) => {\n  it.each([2])('adds %i', (m) => {});\n});\ntest.each`\n  a\n`('table $a', () => {});\n";
    assert_eq!(
        runnables("javascript", src),
        vec![
            r(0, TestModule, "math %i"),
            r(1, Test, "math %i::adds %i"),
            r(3, Test, "table $a"),
        ]
    );
}

#[test]
fn test_titles_keep_whitespace_and_newlines() {
    let src = "it(`multi\n  line`, () => {});\ntest('  padded ', () => {});\n";
    assert_eq!(
        runnables("javascript", src),
        vec![r(0, Test, "multi\n  line"), r(2, Test, "  padded ")]
    );
}

#[test]
fn lines_follow_rope_breaks() {
    // A lone CR is a rope line break but not a tree-sitter row.
    let src = "x = 1\r\ry = 2\n\ndef test_a():\n    pass\n";
    let (text, syntax) = parse("python", src);
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].line,
        text.byte_to_line(src.find("def").expect("def"))
    );
    let symbols = syntax.symbols(&text);
    assert_eq!(symbols[0].range, 4..6);
}

#[test]
fn c_main_in_else_branch_and_nested_conditionals() {
    let src = "#if A\n#ifdef B\nint main(void) { return 0; }\n#endif\n#else\nint main(int c, char **v) { return 1; }\n#endif\nvoid f(void) {}\n";
    assert_eq!(
        runnables("c", src),
        vec![r(2, Main, "main"), r(5, Main, "main")]
    );
}

#[test]
fn c_outline_function_pointer_returns() {
    let src = "int (*getf(void))(int) { return 0; }\nchar ***triple(void) { return 0; }\n";
    assert_eq!(
        outline("c", src),
        vec![o(0, Function, "getf"), o(0, Function, "triple")]
    );
}

#[test]
fn cpp_outline_names_googletest_cases() {
    let src = "TEST(Math, Adds) {\n  EXPECT_EQ(1, 1);\n}\nchar **Pool::take() { return 0; }\n";
    assert_eq!(
        outline("cpp", src),
        vec![o(0, Function, "Math.Adds"), o(0, Method, "Pool::take")]
    );
}

#[test]
fn typescript_declare_module_name_has_no_quotes() {
    let src = "declare module \"foo\" {\n  export function f(): void;\n}\n";
    assert_eq!(
        outline("typescript", src),
        vec![o(0, Module, "foo"), o(1, Function, "f")]
    );
}
