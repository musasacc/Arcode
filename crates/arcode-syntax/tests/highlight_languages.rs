//! Highlighting basics for systems and scripting languages: a small
//! realistic sample per language must produce the expected captures.

mod common;

use common::{BASICS, assert_has, check, highlight_all, parse};

#[test]
fn python() {
    let src = r#"# comment
import os

class Shape(Base):
    """Docstring."""
    def area(self, scale=2):
        return len(self.name) * 3.5 if True else None

print(f"x={x}\n")
"#;
    let names = check(
        "python",
        src,
        &[
            (0, "# comment", "comment"),
            (1, "import", "keyword"),
            (3, "class", "keyword"),
            (3, "Shape", "type"),
            (4, "\"\"\"Docstring.\"\"\"", "comment.documentation"),
            (5, "area", "function"),
            (5, "self", "variable.builtin"),
            (5, "scale", "variable.parameter"),
            (6, "return", "keyword.control"),
            (6, "len", "function.builtin"),
            (6, "name", "property"),
            (6, "3.5", "number"),
            (6, "True", "constant.builtin"),
            (8, "print", "function.builtin"),
            (8, "\\n", "string.escape"),
            (8, "f\"x=", "string"),
        ],
    );
    assert_has(&names, BASICS);
}

#[test]
fn go() {
    let src = r#"package main

// Point is a point.
type Point struct {
	X int
}

func (p *Point) Move(dx int) error {
	fmt.Println("moved\n", len(x), nil)
	return nil
}
"#;
    let names = check(
        "go",
        src,
        &[
            (0, "package", "keyword"),
            (2, "// Point is a point.", "comment"),
            (3, "Point", "type"),
            (3, "struct", "keyword"),
            (4, "X", "property"),
            (4, "int", "type.builtin"),
            (7, "func", "keyword"),
            (7, "Move", "function.method"),
            (7, "dx", "variable.parameter"),
            (7, "error", "type.builtin"),
            (8, "Println", "function.method"),
            (8, "\\n", "string.escape"),
            (8, "len", "function.builtin"),
            (8, "nil", "constant.builtin"),
            (9, "return", "keyword.control"),
        ],
    );
    assert_has(&names, BASICS);
}

const C: &str = r#"#include <stdio.h>
#define MAX 10
/* block */
static int add(int lhs, int rhs) {
    if (lhs > MAX) return 0;
    printf("%d\n", lhs + rhs);
    return 'c';
}
"#;

#[test]
fn c() {
    for id in ["c", "cpp"] {
        let names = check(
            id,
            C,
            &[
                (0, "#include", "keyword"),
                (0, "<stdio.h>", "string"),
                (1, "MAX", "constant"),
                (2, "/* block */", "comment"),
                (3, "static", "keyword"),
                (3, "int", "type.builtin"),
                (3, "add", "function"),
                (3, "lhs", "variable.parameter"),
                (4, "if", "keyword.control"),
                (5, "printf", "function"),
                (5, "\\n", "string.escape"),
                (6, "'c'", "string"),
            ],
        );
        assert_has(&names, BASICS);
    }
}

#[test]
fn cpp() {
    let src = "namespace geo {\nclass Point { public: virtual void move() override; };\nauto p = std::make_unique<Point>(nullptr);\n}\n";
    check(
        "cpp",
        src,
        &[
            (0, "namespace", "keyword"),
            (0, "geo", "module"),
            (1, "class", "keyword"),
            (1, "Point", "type"),
            (1, "virtual", "keyword"),
            (1, "move", "function.method"),
            (2, "auto", "type.builtin"),
            (2, "std", "module"),
            (2, "make_unique", "function"),
            (2, "nullptr", "constant.builtin"),
        ],
    );
}

#[test]
fn bash() {
    let src = "#!/bin/bash\n# comment\nbuild() {\n  local out=\"$HOME/x\"\n  if [ -f \"$out\" ]; then echo 'ok' | grep -q ok; fi\n}\n";
    let names = check(
        "shellscript",
        src,
        &[
            (1, "# comment", "comment"),
            (2, "build", "function"),
            (3, "local", "keyword"),
            (3, "out", "variable"),
            (3, "HOME", "variable.builtin"),
            (4, "if", "keyword.control"),
            (4, "-f", "operator"),
            (4, "echo", "function.builtin"),
            (4, "'ok'", "string"),
            (4, "grep", "function"),
            (4, "-q", "constant"),
            (4, "|", "operator"),
        ],
    );
    assert_has(&names, BASICS);
}

#[test]
fn java() {
    let src = r#"package app;
/** Doc. */
@Deprecated
public class App extends Base {
    // comment
    public static void main(String[] args) {
        System.out.println("hi\n" + 42 + null);
        new App();
    }
}
"#;
    let names = check(
        "java",
        src,
        &[
            (0, "package", "keyword"),
            (1, "/** Doc. */", "comment.documentation"),
            (2, "Deprecated", "attribute"),
            (3, "public", "keyword"),
            (3, "App", "type"),
            (4, "// comment", "comment"),
            (5, "void", "type.builtin"),
            (5, "main", "function.method"),
            (5, "String", "type"),
            (5, "args", "variable.parameter"),
            (6, "System", "type"),
            (6, "println", "function.method"),
            (6, "\\n", "string.escape"),
            (6, "42", "number"),
            (6, "null", "constant.builtin"),
            (7, "App", "constructor"),
        ],
    );
    assert_has(&names, &["keyword", "function", "string", "comment"]);
}

#[test]
fn lua() {
    let src = "-- comment\nlocal function greet(name)\n  if name ~= nil then print(\"hi \" .. name) end\n  return M.helper(true, 3)\nend\n";
    let names = check(
        "lua",
        src,
        &[
            (0, "-- comment", "comment"),
            (1, "local", "keyword"),
            (1, "function", "keyword"),
            (1, "greet", "function"),
            (1, "name", "variable.parameter"),
            (2, "if", "keyword.control"),
            (2, "~=", "operator"),
            (2, "nil", "constant.builtin"),
            (2, "print", "function.builtin"),
            (2, "\"hi \"", "string"),
            (3, "return", "keyword.control"),
            (3, "helper", "function"),
            (3, "true", "boolean"),
            (3, "3", "number"),
        ],
    );
    assert_has(&names, BASICS);
}

#[test]
fn ruby() {
    let src = "# comment\nrequire 'json'\nclass Shape < Base\n  def area(scale)\n    @w * scale if :sym == nil\n    puts \"v=#{x}\"\n  end\nend\n";
    let names = check(
        "ruby",
        src,
        &[
            (0, "# comment", "comment"),
            (1, "require", "function.builtin"),
            (1, "'json'", "string"),
            (2, "class", "keyword"),
            (2, "Shape", "type"),
            (3, "def", "keyword"),
            (3, "area", "function"),
            (3, "scale", "variable.parameter"),
            (4, "@w", "property"),
            (4, "if", "keyword.control"),
            (4, ":sym", "string.special"),
            (4, "nil", "constant.builtin"),
            (5, "#{", "punctuation.special"),
            (7, "end", "keyword"),
        ],
    );
    assert_has(&names, BASICS);
}

#[test]
fn every_grammar_highlights_its_own_sample_without_errors() {
    let samples = [
        ("python", "def f():\n    return 1\n"),
        ("javascript", "function f() { return 1; }\n"),
        ("typescript", "function f(): number { return 1; }\n"),
        ("go", "package x\nfunc f() int { return 1 }\n"),
        ("c", "int f(void) { return 1; }\n"),
        ("cpp", "int f() { return 1; }\n"),
        ("shellscript", "f() { return 1; }\n"),
        ("java", "class A { int f() { return 1; } }\n"),
        ("lua", "function f() return 1 end\n"),
        ("ruby", "def f\n  1\nend\n"),
        ("php", "<?php function f() { return 1; }\n"),
    ];
    for (id, src) in samples {
        let (text, syntax) = parse(id, src);
        assert!(!syntax.tree().root_node().has_error(), "{id}");
        let lines = highlight_all(&syntax, &text);
        assert!(
            lines
                .iter()
                .flatten()
                .any(|s| s.name.starts_with("function")),
            "{id}"
        );
    }
}
