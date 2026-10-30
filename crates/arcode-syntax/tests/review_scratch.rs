mod common;

use std::time::Instant;

use arcode_syntax::all_languages;
use common::parse;
use ropey::Rope;

fn show(id: &str, src: &str) {
    let (text, syntax) = parse(id, src);
    println!("--- {id}\n{src}");
    for r in syntax.runnables(&text) {
        println!("RUN {:?}", r);
    }
    for s in syntax.symbols(&text) {
        println!("SYM {} {:?} {:?} {:?} d{}", s.name, s.kind, s.range, s.name_range, s.depth);
    }
}

#[test]
fn scratch_cases() {
    show("go", "package main\nconst a, b = 1, 2\nconst (\n  X, Y = 1, 2\n)\nvar v = 1\nfunc main() {}\n");
    show("c", "#ifdef MAIN\nint main(void) { return 0; }\n#endif\nchar **dup(void) { return 0; }\nint (*getf(void))(int) { return 0; }\n");
    show("cpp", "#include <gtest/gtest.h>\nTEST(Math, Adds) { EXPECT_EQ(1, 1); }\nnamespace a { int main() { return 0; } }\nint main(int argc, char** argv) { return 0; }\n");
    show("python", "if '__main__' == __name__:\n    pass\nclass TestA:\n    class TestB:\n        def test_x(self): pass\n");
    show("rust", "#[cfg(all(test, feature = \"x\"))]\nmod t {\n    #[test]\n    fn a() {}\n}\n#![cfg(test)]\n#[rstest]\nfn b() {}\n#[test_case(1)]\nfn c(x: u8) {}\n");
    show("rust", "mod outer {\n    mod tests;\n    #[test] fn x() {}\n}\n");
    show("markdown", "# Title\n\ntext\n\n## Sub ##\n\nmore\n\nSetext\n======\n\n```rust\nfn x() {}\n```\n");
    show("typescript", "declare module \"foo\" {\n  export function f(): void;\n}\nexport default class { m() {} }\ntest.describe('pw', () => { test('a', async () => {}); });\n");
    show("javascript", "describe.each([1])('each %i', (n) => { it('x', () => {}); });\nit.each([1])('e %i', () => {});\n");
    show("go", "package foo\nfunc TestX(t *testing.T) {}\nfunc (s *Srv[T]) Get() {}\ntype I interface { M() }\n");
    show("java", "class A { @Test void testX() {} public static void main(String[] a) {} }\n");
}

#[test]
fn rust_many_attributes_cost() {
    for n in [100usize, 200, 400, 800] {
        let mut src = String::new();
        for _ in 0..n {
            src.push_str("#[allow(dead_code)]\n");
        }
        src.push_str("fn f() {}\n");
        let (text, syntax) = parse("rust", &src);
        let _ = syntax.runnables(&Rope::from_str("")); // warm query
        let t = Instant::now();
        let items = syntax.runnables(&text);
        println!("{n} attrs: {:?} items {}", t.elapsed(), items.len());
    }
}

#[test]
fn scope_quadratic() {
    for blocks in [2_000usize, 4_000, 8_000, 16_000] {
        let mut src = String::new();
        for i in 0..blocks {
            src.push_str(&format!("describe('s{i}', () => {{\n  it('t{i}', () => {{}});\n}});\n"));
        }
        let (text, syntax) = parse("javascript", &src);
        let _ = syntax.runnables(&Rope::from_str(""));
        let t = Instant::now();
        let items = syntax.runnables(&text);
        println!("{blocks} blocks: {:?} items {}", t.elapsed(), items.len());
    }
    for mods in [1_000usize, 8_000] {
        let mut src = String::new();
        for i in 0..mods {
            src.push_str(&format!("mod m{i} {{\n    #[test]\n    fn t() {{}}\n}}\n"));
        }
        let (text, syntax) = parse("rust", &src);
        let t = Instant::now();
        let items = syntax.runnables(&text);
        println!("{mods} rust mods: {:?} items {}", t.elapsed(), items.len());
    }
}

/// Pseudo-random fuzz over all grammars: parse, highlight, outline, runnables
/// and incremental edits must not panic.
#[test]
fn fuzz_all_languages() {
    let pieces = [
        "fn ", "def ", "class ", "{", "}", "(", ")", "[", "]", "\"", "'", "`", "\n", "\r\n", "\r",
        "\u{2028}", "é", "🦀", "#", "##", "---\n", "```", "<!--", "-->", "<?php", "?>", "<div>",
        "</div>", "${", "/*", "*/", "//", "--[[", "]]", "=begin\n", "=end\n", "  ", "\t", "test",
        "describe('a', ", "it(", "#[test]", "mod m {", "func ", "package main\n", ": ", "- ",
        "|\n", "&a ", "*a ", "!!", "%", "@", "\\", "\0", "<<EOF\n", "EOF\n", "if __name__ == \"__main__\":\n",
    ];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for lang in all_languages() {
        if !lang.has_grammar() {
            continue;
        }
        for _ in 0..60 {
            let n = (next() % 200) as usize;
            let src: String = (0..n).map(|_| pieces[(next() % pieces.len() as u64) as usize]).collect();
            let mut text = Rope::from_str(&src);
            let Some(mut syntax) = arcode_syntax::Syntax::new(lang, &text) else { continue };
            let _ = syntax.highlight_lines(&text, 0..text.len_lines() + 2);
            let _ = syntax.symbols(&text);
            let _ = syntax.runnables(&text);
            for _ in 0..5 {
                let len = text.len_chars();
                let a = (next() as usize) % (len + 1);
                let b = (a + (next() as usize) % 10).min(len);
                let start = text.char_to_byte(a);
                let end = text.char_to_byte(b);
                let ins = pieces[(next() % pieces.len() as u64) as usize];
                let edit = common::apply_edit(&mut text, start..end, ins);
                syntax.edit(&edit);
                syntax.reparse(&text);
                let _ = syntax.highlight_lines(&text, 0..text.len_lines());
                let _ = syntax.symbols(&text);
                let _ = syntax.runnables(&text);
                for b in 0..text.len_bytes().min(50) {
                    let _ = syntax.matching_bracket(&text, b);
                }
            }
        }
    }
}
