//! Outline symbols: C, C++, Markdown, Java, shell, and query sanity checks.

mod common;

use arcode_syntax::OutlineKind::{
    Class, Constructor, Enum, Function, Heading, Interface, Macro, Method, Module, Struct, Type,
};
use arcode_syntax::{OutlineKind, all_languages};
use common::{o, outline, parse};

#[test]
fn c_outline() {
    let src = r#"#include <stdio.h>
#define MAX(a, b) ((a) > (b) ? (a) : (b))

struct point {
    int x;
};

typedef struct {
    int w;
} size_t2;

enum color { RED, GREEN };

static char *name(void) { return "x"; }

int main(void) {
    return 0;
}
"#;
    assert_eq!(
        outline("c", src),
        vec![
            o(0, Macro, "MAX"),
            o(0, Struct, "point"),
            o(0, Type, "size_t2"),
            o(0, Enum, "color"),
            o(0, Function, "name"),
            o(0, Function, "main"),
        ]
    );
}

#[test]
fn cpp_outline() {
    let src = r#"namespace geo {

class Point {
public:
    Point();
    ~Point();
    int x() const { return x_; }
    void move(int dx);
private:
    int x_;
};

struct Size { int w; };

void Point::move(int dx) { x_ += dx; }

}

using Id = int;

int main() { return 0; }
"#;
    assert_eq!(
        outline("cpp", src),
        vec![
            o(0, Module, "geo"),
            o(1, Class, "Point"),
            o(2, Method, "Point"),
            o(2, Method, "~Point"),
            o(2, Method, "x"),
            o(2, Method, "move"),
            o(1, Struct, "Size"),
            o(1, Method, "Point::move"),
            o(0, Type, "Id"),
            o(0, Function, "main"),
        ]
    );
}

#[test]
fn markdown_outline_nests_sections() {
    let src = "# Title\n\nIntro.\n\n## Install\n\nText.\n\n### From source\n\nMore.\n\n## Usage\n\nSetext\n------\n";
    assert_eq!(
        outline("markdown", src),
        vec![
            o(0, Heading, "Title"),
            o(1, Heading, "Install"),
            o(2, Heading, "From source"),
            o(1, Heading, "Usage"),
            // A level-2 setext heading is a sibling of `## Usage`.
            o(1, Heading, "Setext"),
        ]
    );
}

#[test]
fn markdown_setext_headings_open_sections() {
    let src = "## Sub ##\n\ntext\n\nTop\n===\n\n### Deep\n\nNext\n----\n";
    assert_eq!(
        outline("markdown", src),
        vec![
            o(0, Heading, "Sub"),
            o(0, Heading, "Top"),
            o(1, Heading, "Deep"),
            o(1, Heading, "Next"),
        ]
    );
    let (text, syntax) = common::parse("markdown", src);
    let ranges: Vec<_> = syntax.symbols(&text).into_iter().map(|i| i.range).collect();
    assert_eq!(ranges, [0..4, 4..11, 7..9, 9..11]);
}

#[test]
fn java_and_bash_outline() {
    let java = "public class App {\n  public App() {}\n  public static void main(String[] args) {}\n  interface Listener { void on(); }\n  enum Mode { A }\n}\n";
    assert_eq!(
        outline("java", java),
        vec![
            o(0, Class, "App"),
            o(1, Constructor, "App"),
            o(1, Method, "main"),
            o(1, Interface, "Listener"),
            o(2, Method, "on"),
            o(1, Enum, "Mode"),
        ]
    );
    let sh = "#!/bin/sh\nsetup() {\n  echo hi\n}\nfunction build {\n  make\n}\n";
    assert_eq!(
        outline("shellscript", sh),
        vec![o(0, Function, "setup"), o(0, Function, "build")]
    );
}

#[test]
fn no_outline_for_languages_without_query() {
    assert!(outline("toml", "[a]\nb = 1\n").is_empty());
    assert!(outline("json", "{\"a\": 1}").is_empty());
}

#[test]
fn outline_queries_use_known_kinds() {
    for lang in all_languages() {
        if lang.outline_query.is_empty() {
            continue;
        }
        let query = match lang.compile_outline_query() {
            Ok(query) => query,
            Err(err) => panic!("{err}"),
        };
        for pattern in 0..query.pattern_count() {
            let kind = query
                .property_settings(pattern)
                .iter()
                .find(|prop| &*prop.key == "kind")
                .and_then(|prop| prop.value.as_deref());
            let Some(kind) = kind else {
                panic!("{}: pattern {pattern} has no kind", lang.name);
            };
            assert!(
                OutlineKind::from_query_name(kind).is_some(),
                "{}: unknown kind {kind}",
                lang.name
            );
        }
    }
}

#[test]
fn outline_on_broken_code_does_not_panic() {
    for lang in all_languages().iter().filter(|l| l.has_grammar()) {
        let (text, syntax) = parse(lang.id, "fn ( { class def struct } ) ]] \"unterminated\n#");
        let _ = syntax.symbols(&text);
        let _ = syntax.runnables(&text);
    }
}
