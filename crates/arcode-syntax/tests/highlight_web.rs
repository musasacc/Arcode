//! Highlighting basics for web and markup languages (JS/TS/JSX/TSX, HTML,
//! CSS, YAML, Markdown, PHP).

mod common;

use common::{BASICS, assert_has, check};

const JS: &str = r#"// comment
import fs from "fs";
export async function load(path, opts = {}) {
  const re = /a+b/g;
  return fs.readFile(`${path}/x`, 42) ?? null;
}
class Store extends Base {}
"#;

#[test]
fn javascript() {
    for id in [
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
    ] {
        let names = check(
            id,
            JS,
            &[
                (0, "// comment", "comment"),
                (1, "import", "keyword"),
                (1, "\"fs\"", "string"),
                (2, "async", "keyword"),
                (2, "load", "function"),
                (2, "path", "variable.parameter"),
                (3, "const", "keyword"),
                (3, "/a+b/g", "string.regex"),
                (4, "return", "keyword.control"),
                (4, "readFile", "function.method"),
                (4, "${", "punctuation.special"),
                (4, "42", "number"),
                (4, "??", "operator"),
                (4, "null", "constant.builtin"),
                (6, "Store", "type"),
            ],
        );
        assert_has(&names, BASICS);
    }
}

#[test]
fn jsx_and_tsx() {
    let src = "const App = () => <div className=\"x\"><Child /></div>;\n";
    for id in ["javascript", "javascriptreact", "typescriptreact"] {
        check(
            id,
            src,
            &[
                (0, "div", "tag"),
                (0, "className", "attribute"),
                (0, "Child", "constructor"),
                (0, "\"x\"", "string"),
            ],
        );
    }
}

#[test]
fn typescript() {
    let src =
        "interface Shape { area(): number; }\ntype Id = string;\nfunction f(x: Shape): void {}\n";
    for id in ["typescript", "typescriptreact"] {
        check(
            id,
            src,
            &[
                (0, "interface", "keyword"),
                (0, "Shape", "type"),
                (0, "area", "function.method"),
                (0, "number", "type.builtin"),
                (1, "type", "keyword"),
                (1, "Id", "type"),
                (2, "x", "variable.parameter"),
                (2, "void", "type.builtin"),
            ],
        );
    }
}

#[test]
fn html() {
    let src = "<!DOCTYPE html>\n<!-- note -->\n<a href=\"/x\" class=main>Link &amp; more</a>\n";
    check(
        "html",
        src,
        &[
            (0, "DOCTYPE html", "constant"),
            (1, "<!-- note -->", "comment"),
            (2, "a", "tag"),
            (2, "href", "attribute"),
            (2, "\"/x\"", "string"),
            (2, "main", "string"),
            (2, "&amp;", "string.escape"),
            (2, "</", "punctuation.bracket"),
        ],
    );
}

#[test]
fn css() {
    let src = "/* c */\n@media screen {\n  .card > li:hover { color: #fff; margin: 4px !important; --gap: calc(1em + 2px); }\n}\n";
    let names = check(
        "css",
        src,
        &[
            (0, "/* c */", "comment"),
            (1, "@media", "keyword"),
            (2, "card", "type"),
            (2, "li", "tag"),
            (2, "hover", "attribute"),
            (2, "color", "property"),
            (2, "#fff", "string.special"),
            (2, "4", "number"),
            (2, "px", "type.builtin"),
            (2, "!important", "keyword"),
            (2, "--gap", "variable"),
            (2, "calc", "function"),
        ],
    );
    assert_has(&names, &["keyword", "function", "comment"]);
}

#[test]
fn yaml() {
    let src = "# comment\nname: \"arcode\"\nversion: 1.5\nenabled: true\nnone: null\nlist:\n  - &a item\n  - *a\n";
    let names = check(
        "yaml",
        src,
        &[
            (0, "# comment", "comment"),
            (1, "name", "property"),
            (1, ":", "punctuation.delimiter"),
            (1, "\"arcode\"", "string"),
            (2, "1.5", "number"),
            (3, "true", "boolean"),
            (4, "null", "constant.builtin"),
            (6, "-", "punctuation.delimiter"),
            (6, "&", "punctuation.special"),
            (6, "item", "string"),
            (7, "a", "label"),
        ],
    );
    assert_has(&names, &["string", "comment"]);
}

#[test]
fn markdown() {
    let src =
        "# Title\n\n- item\n\n```rust\nfn main() {}\n```\n\n[link]: https://example.com\n> quote\n";
    check(
        "markdown",
        src,
        &[
            (0, "#", "punctuation.special"),
            (0, "Title", "keyword"),
            (2, "-", "punctuation.special"),
            (4, "```", "punctuation.delimiter"),
            (4, "rust", "label"),
            (5, "fn main() {}", "string"),
            (8, "[link]", "label"),
            (8, "https://example.com", "string.special"),
            (9, ">", "punctuation.special"),
        ],
    );
}

#[test]
fn php() {
    let src = "<?php\n// comment\nnamespace App;\nclass User extends Model {\n  public function name(int $id): string {\n    return strlen(\"x\") + $this->count;\n  }\n}\n";
    let names = check(
        "php",
        src,
        &[
            (0, "<?php", "tag"),
            (1, "// comment", "comment"),
            (2, "namespace", "keyword"),
            (2, "App", "module"),
            (3, "class", "keyword"),
            (3, "User", "type"),
            (4, "public", "keyword"),
            (4, "name", "function.method"),
            (4, "int", "type.builtin"),
            (4, "$id", "variable.parameter"),
            (5, "return", "keyword.control"),
            (5, "strlen", "function"),
            (5, "\"x\"", "string"),
            (5, "this", "variable.builtin"),
            (5, "count", "property"),
        ],
    );
    assert_has(&names, BASICS);
}
