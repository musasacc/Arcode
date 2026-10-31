//! Runnables (mains, tests, test modules, benchmarks) per language.

mod common;

use arcode_syntax::RunnableKind::{Bench, Main, Test, TestModule};
use arcode_syntax::{RunnableKind, all_languages};
use common::parse;

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
fn rust_main_tests_modules_and_benches() {
    let src = r#"use std::io;

fn main() {
    println!("hi");
}

fn helper() {}

#[test]
fn top_level() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {}

    #[test]
    #[should_panic(expected = "boom")]
    fn panics() {}

    #[should_panic]
    // a comment between attributes
    #[test]
    fn attr_after() {}

    #[tokio::test(flavor = "multi_thread")]
    async fn async_test() {}

    fn not_a_test() {}

    mod nested {
        #[test]
        fn deep() {}
    }

    #[bench]
    fn bench_add(b: &mut Bencher) {}
}

mod outer {
    #[cfg(test)]
    mod tests {
        #[actix_web::test]
        async fn in_outer() {}
    }

    fn main() {}
}
"#;
    assert_eq!(
        runnables("rust", src),
        vec![
            r(2, Main, "main"),
            r(9, Test, "top_level"),
            r(12, TestModule, "tests"),
            r(16, Test, "tests::adds"),
            r(20, Test, "tests::panics"),
            r(25, Test, "tests::attr_after"),
            r(28, Test, "tests::async_test"),
            r(34, Test, "tests::nested::deep"),
            r(38, Bench, "tests::bench_add"),
            r(43, TestModule, "outer::tests"),
            r(45, Test, "outer::tests::in_outer"),
        ]
    );
}

#[test]
fn rust_scope_path_is_exposed() {
    let src = "mod a {\n    mod b {\n        #[test]\n        fn t() {}\n    }\n}\n";
    let (text, syntax) = parse("rust", src);
    let items = syntax.runnables(&text);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "t");
    assert_eq!(items[0].scope_path, ["a", "b"]);
    assert_eq!(items[0].line, 3);
}

#[test]
fn rust_cfg_other_than_test_is_not_a_test_module() {
    let src =
        "#[cfg(unix)]\nmod unix {}\n#[cfg(not(test))]\nmod real {}\n#[derive(Debug)]\nstruct S;\n";
    assert_eq!(runnables("rust", src), vec![]);
}

#[test]
fn python_main_tests_and_classes() {
    let src = r#"import unittest

def helper():
    pass

def test_top():
    assert helper() is None

@pytest.mark.slow
def test_decorated():
    pass

class TestMath:
    def test_add(self):
        pass

    def setup_method(self):
        pass

    @pytest.mark.parametrize("x", [1, 2])
    def test_param(self, x):
        pass

class Helper:
    def test_not_collected(self):
        pass

class MathCase(unittest.TestCase):
    def test_sub(self):
        self.assertEqual(1, 1)

if __name__ == "__main__":
    unittest.main()
"#;
    assert_eq!(
        runnables("python", src),
        vec![
            r(5, Test, "test_top"),
            r(9, Test, "test_decorated"),
            r(12, TestModule, "TestMath"),
            r(13, Test, "TestMath::test_add"),
            r(20, Test, "TestMath::test_param"),
            r(27, TestModule, "MathCase"),
            r(28, Test, "MathCase::test_sub"),
            r(31, Main, "__main__"),
        ]
    );
}

#[test]
fn python_main_needs_module_level_dunder_check() {
    let src = "def f():\n    if __name__ == \"__main__\":\n        pass\nif __name__ == 'other':\n    pass\n";
    assert_eq!(runnables("python", src), vec![]);
}

const JS_TESTS: &str = r#"import { describe, it, expect } from "vitest";

describe("math", () => {
  it("adds", () => {
    expect(1 + 1).toBe(2);
  });

  describe.skip('nested suite', () => {
    test.only(`template name`, () => {});
  });

  helper("not a test", () => {});
});

test("top level", async () => {});
it(variableName, () => {});
bench("fast path", () => {});
"#;

#[test]
fn javascript_describe_it_test() {
    let expected = vec![
        r(2, TestModule, "math"),
        r(3, Test, "math::adds"),
        r(7, TestModule, "math::nested suite"),
        r(8, Test, "math::nested suite::template name"),
        r(14, Test, "top level"),
        r(16, Bench, "fast path"),
    ];
    for id in [
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
    ] {
        assert_eq!(runnables(id, JS_TESTS), expected, "{id}");
    }
}

#[test]
fn typescript_tests_with_types() {
    let src = "describe('Parser', () => {\n  it('parses <T>', (): void => {\n    const x: number = 1;\n  });\n});\n";
    assert_eq!(
        runnables("typescript", src),
        vec![r(0, TestModule, "Parser"), r(1, Test, "Parser::parses <T>")]
    );
}

#[test]
fn go_main_tests_and_benchmarks() {
    let main = "package main\n\nimport \"fmt\"\n\nfunc helper() {}\n\nfunc main() {\n\tfmt.Println(\"hi\")\n}\n";
    assert_eq!(runnables("go", main), vec![r(6, Main, "main")]);

    let lib = "package mathx\n\nfunc main() {}\n";
    assert_eq!(runnables("go", lib), vec![]);

    let tests = r#"package mathx

import "testing"

func TestAdd(t *testing.T) {
	t.Run("sub", func(t *testing.T) {})
}

func Test_underscore(t *testing.T) {}

func Testing(t *testing.T) {}

func TestHelper(x int) {}

func BenchmarkAdd(b *testing.B) {
	for i := 0; i < b.N; i++ {}
}

func FuzzParse(f *testing.F) {}

func TestMain(m *testing.M) {}
"#;
    assert_eq!(
        runnables("go", tests),
        vec![
            r(4, Test, "TestAdd"),
            r(8, Test, "Test_underscore"),
            r(14, Bench, "BenchmarkAdd"),
            r(18, Test, "FuzzParse"),
        ]
    );
}

#[test]
fn c_and_cpp_main() {
    let c = "#include <stdio.h>\n\nstatic int helper(void) { return 1; }\n\nint main(int argc, char **argv) {\n    return helper();\n}\n";
    assert_eq!(runnables("c", c), vec![r(4, Main, "main")]);
    assert_eq!(runnables("cpp", c), vec![r(4, Main, "main")]);

    let not_main = "void main_loop(void) {}\nvoid main(void) {}\n";
    assert_eq!(runnables("c", not_main), vec![]);
}

#[test]
fn cpp_googletest_cases() {
    let src = "#include <gtest/gtest.h>\n\nTEST(MathTest, Adds) {\n  EXPECT_EQ(2, 1 + 1);\n}\n\nTEST_F(Fixture, UsesFixture) {}\n\nint main(int argc, char** argv) {\n  return RUN_ALL_TESTS();\n}\n";
    assert_eq!(
        runnables("cpp", src),
        vec![
            r(2, Test, "MathTest.Adds"),
            r(6, Test, "Fixture.UsesFixture"),
            r(8, Main, "main"),
        ]
    );
}

#[test]
fn languages_without_runnables_return_nothing() {
    assert_eq!(runnables("toml", "[package]\nname = \"main\"\n"), vec![]);
    assert_eq!(runnables("shellscript", "main() { echo; }\nmain\n"), vec![]);
}

#[test]
fn runnables_queries_use_known_kinds() {
    for lang in all_languages() {
        if lang.runnables_query.is_empty() {
            continue;
        }
        let query = match lang.compile_runnables_query() {
            Ok(query) => query,
            Err(err) => panic!("{err}"),
        };
        for pattern in 0..query.pattern_count() {
            let kind = query
                .property_settings(pattern)
                .iter()
                .find(|prop| &*prop.key == "kind")
                .and_then(|prop| prop.value.as_deref());
            if let Some(kind) = kind {
                assert!(
                    RunnableKind::from_query_name(kind).is_some(),
                    "{}: unknown kind {kind}",
                    lang.name
                );
            }
        }
    }
}

#[test]
fn edits_update_runnables() {
    let src = "fn main() {}\n";
    let (mut text, mut syntax) = parse("rust", src);
    assert_eq!(syntax.runnables(&text).len(), 1);
    let end = text.len_bytes();
    let edit = common::apply_edit(&mut text, end..end, "#[test]\nfn t() {}\n");
    syntax.edit(&edit);
    syntax.reparse(&text);
    let kinds: Vec<_> = syntax.runnables(&text).iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [Main, Test]);
}
