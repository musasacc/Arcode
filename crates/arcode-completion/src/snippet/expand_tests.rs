#![allow(clippy::single_range_in_vec_init)]

use std::path::PathBuf;

use super::*;
use crate::snippet::variables::DateTime;

fn ex(s: &str) -> ExpandedSnippet {
    expand(s, &VariableContext::default(), "", "    ")
}

fn stop(e: &ExpandedSnippet, index: u32) -> &Tabstop {
    e.tabstops
        .iter()
        .find(|t| t.index == index)
        .expect("tabstop")
}

fn indices(e: &ExpandedSnippet) -> Vec<u32> {
    e.tabstops.iter().map(|t| t.index).collect()
}

#[test]
fn simple_placeholders_and_order() {
    let e = ex("fn ${1:name}($2) {$0}");
    assert_eq!(e.text, "fn name() {}");
    assert_eq!(indices(&e), [1, 2, 0]);
    assert_eq!(stop(&e, 1).ranges, [3..7]);
    assert_eq!(stop(&e, 2).ranges, [8..8]);
    assert_eq!(stop(&e, 0).ranges, [11..11]);
}

#[test]
fn implicit_final_tabstop() {
    let e = ex("a${2:x}b$1");
    assert_eq!(e.text, "axb");
    assert_eq!(indices(&e), [1, 2, 0]);
    assert_eq!(stop(&e, 0).ranges, [3..3]);
    let e = ex("");
    assert_eq!(indices(&e), [0]);
    assert_eq!(e.tabstops[0].ranges, [0..0]);
}

#[test]
fn mirrors_share_text() {
    let e = ex("$1 = ${1:val}; use($1)");
    assert_eq!(e.text, "val = val; use(val)");
    assert_eq!(stop(&e, 1).ranges, [0..3, 6..9, 15..18]);
}

#[test]
fn nested_placeholders() {
    let e = ex("${1:outer ${2:inner}}!");
    assert_eq!(e.text, "outer inner!");
    assert_eq!(stop(&e, 1).ranges, [0..11]);
    assert_eq!(stop(&e, 2).ranges, [6..11]);
}

#[test]
fn choices_insert_first() {
    let e = ex("let x = ${1|true,false|};");
    assert_eq!(e.text, "let x = true;");
    let t = stop(&e, 1);
    assert_eq!(t.ranges, [8..12]);
    assert_eq!(
        t.choices.as_deref(),
        Some(&["true".to_string(), "false".to_string()][..])
    );
}

#[test]
fn indentation_and_tabs() {
    let e = expand("if x {\n\t$0\n}", &VariableContext::default(), "  ", "    ");
    assert_eq!(e.text, "if x {\n      \n  }");
    assert_eq!(stop(&e, 0).ranges, [13..13]);
    // Tabs after content are kept.
    let e = expand("a\tb\n\tc\td", &VariableContext::default(), "", "  ");
    assert_eq!(e.text, "a\tb\n  c\td");
}

#[test]
fn multiline_placeholder_mirrors_get_indent() {
    let e = expand("${1:a\nb}\n$1", &VariableContext::default(), ">", "\t");
    assert_eq!(e.text, "a\n>b\n>a\n>b");
    assert_eq!(stop(&e, 1).ranges, [0..4, 6..10]);
}

#[test]
fn known_variables_resolve() {
    let ctx = VariableContext {
        file_path: Some(PathBuf::from("/x/lib.rs")),
        selected_text: String::new(),
        now: Some(DateTime::from_unix(1_709_210_096)),
        ..Default::default()
    };
    let e = expand(
        "$TM_FILENAME_BASE-${CURRENT_YEAR}: ${TM_SELECTED_TEXT:${1:none}}",
        &ctx,
        "",
        "\t",
    );
    assert_eq!(e.text, "lib-2024: none");
    assert_eq!(stop(&e, 1).ranges, [10..14]);
}

#[test]
fn variable_transform() {
    let ctx = VariableContext {
        file_path: Some(PathBuf::from("my_file.rs")),
        ..Default::default()
    };
    let e = expand(
        "${TM_FILENAME/(\\w+)\\.rs/${1:/pascalcase}/}",
        &ctx,
        "",
        "\t",
    );
    assert_eq!(e.text, "MyFile");
}

#[test]
fn unknown_variables_become_placeholders() {
    let e = ex("${2:a} $FOO $BAR ${FOO} ${BAZ:dflt}");
    assert_eq!(e.text, "a FOO BAR FOO dflt");
    assert_eq!(indices(&e), [2, 3, 4, 5, 0]);
    assert_eq!(stop(&e, 3).ranges, [2..5, 10..13]);
    assert_eq!(stop(&e, 4).ranges, [6..9]);
    assert_eq!(stop(&e, 5).ranges, [14..18]);
}

#[test]
fn transformed_tabstop_is_filled_not_tracked() {
    let e = ex("${1:hello} ${1/(.*)/${1:/upcase}/}");
    assert_eq!(e.text, "hello HELLO");
    assert_eq!(stop(&e, 1).ranges, [0..5]);
}

#[test]
fn escapes_and_unicode_offsets_are_chars() {
    let e = ex("ä\\$ö${1:ü}\\}");
    assert_eq!(e.text, "ä$öü}");
    assert_eq!(stop(&e, 1).ranges, [3..4]);
}

#[test]
fn final_placeholder_text() {
    let e = ex("x${0:end}y");
    assert_eq!(e.text, "xendy");
    assert_eq!(indices(&e), [0]);
    assert_eq!(stop(&e, 0).ranges, [1..4]);
}

#[test]
fn self_referencing_placeholder_terminates() {
    let e = ex("${1:a$1}");
    assert!(e.text.starts_with('a'));
    let e = ex("${1:${2:x}} ${2:$1}");
    assert!(!e.text.is_empty());
}
