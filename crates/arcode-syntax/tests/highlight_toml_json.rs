//! TOML and JSON highlighting basics.

mod common;

use common::{highlight_all, name_of, parse, pieces};

#[test]
fn toml_basics() {
    let src = "\
# top comment
[package]
name = \"arcode\" # trailing
version = 1
ratio = 0.5
enabled = true
date = 1979-05-27
list = [1, 2]

[dependencies.foo]
path = '../foo'

[[bin]]
name = \"x\"
";
    let (text, syntax) = parse("toml", src);
    let lines = highlight_all(&syntax, &text);
    let at = |line, needle| name_of(&text, &lines, line, needle);

    assert_eq!(at(0, "# top comment"), Some("comment"));
    assert_eq!(at(1, "["), Some("punctuation.bracket"));
    assert_eq!(at(1, "package"), Some("type"));
    assert_eq!(at(2, "name"), Some("property"));
    assert_eq!(at(2, "="), Some("operator"));
    assert_eq!(at(2, "\"arcode\""), Some("string"));
    assert_eq!(at(2, "# trailing"), Some("comment"));
    assert_eq!(at(3, "1"), Some("number"));
    assert_eq!(at(4, "0.5"), Some("number"));
    assert_eq!(at(5, "true"), Some("boolean"));
    assert_eq!(at(6, "1979-05-27"), Some("string.special"));
    assert_eq!(at(7, ","), Some("punctuation.delimiter"));
    assert_eq!(at(9, "dependencies"), Some("type"));
    assert_eq!(at(9, "foo"), Some("type"));
    assert_eq!(at(10, "'../foo'"), Some("string"));
    assert_eq!(at(12, "[["), Some("punctuation.bracket"));
    assert_eq!(at(12, "bin"), Some("type"));
}

#[test]
fn toml_multiline_string() {
    let src = "s = \"\"\"\nline one\nline two\"\"\"\nk = 1\n";
    let (text, syntax) = parse("toml", src);
    let lines = highlight_all(&syntax, &text);
    assert_eq!(
        pieces(&text, 1, &lines[1]),
        vec![("line one".to_string(), "string")]
    );
    assert_eq!(name_of(&text, &lines, 2, "line two\"\"\""), Some("string"));
    assert_eq!(name_of(&text, &lines, 3, "k"), Some("property"));
}

#[test]
fn json_basics() {
    let src = r#"{
  "key": "value",
  "n": -1.5e3,
  "b": true,
  "f": false,
  "z": null,
  "esc": "a\"b",
  "arr": [1, {"x": "ü"}]
}
"#;
    let (text, syntax) = parse("json", src);
    let lines = highlight_all(&syntax, &text);
    let at = |line, needle| name_of(&text, &lines, line, needle);

    assert_eq!(at(0, "{"), Some("punctuation.bracket"));
    assert_eq!(at(1, "\"key\""), Some("property"));
    assert_eq!(at(1, ":"), Some("punctuation.delimiter"));
    assert_eq!(at(1, "\"value\""), Some("string"));
    assert_eq!(at(1, ","), Some("punctuation.delimiter"));
    assert_eq!(at(2, "-1.5e3"), Some("number"));
    assert_eq!(at(3, "true"), Some("boolean"));
    assert_eq!(at(4, "false"), Some("boolean"));
    assert_eq!(at(5, "null"), Some("constant.builtin"));
    assert_eq!(at(6, "\\\""), Some("string.escape"));
    assert_eq!(at(7, "["), Some("punctuation.bracket"));
    assert_eq!(at(7, "\"x\""), Some("property"));
    assert_eq!(at(7, "\"ü\""), Some("string"));
    assert_eq!(at(8, "}"), Some("punctuation.bracket"));
}

#[test]
fn json_property_escape_is_split() {
    let (text, syntax) = parse("json", "{\"a\\nb\": 1}");
    let lines = highlight_all(&syntax, &text);
    let got = pieces(&text, 0, &lines[0]);
    assert!(got.contains(&("\"a".to_string(), "property")));
    assert!(got.contains(&("\\n".to_string(), "string.escape")));
    assert!(got.contains(&("b\"".to_string(), "property")));
}
