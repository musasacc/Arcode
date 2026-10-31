use super::*;

const SIMPLE: &str =
    "top\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> feature\nbottom\n";

#[test]
fn parses_two_way_conflict() {
    let conflicts = parse_conflicts(SIMPLE);
    assert_eq!(
        conflicts,
        vec![Conflict {
            start_line: 1,
            ours: 2..4,
            base: None,
            theirs: 5..6,
            end_line: 6,
            ours_label: "HEAD".into(),
            base_label: None,
            theirs_label: "feature".into(),
        }]
    );
    assert_eq!(conflicts[0].lines(), 1..7);
}

#[test]
fn resolves_each_way() {
    let c = &parse_conflicts(SIMPLE)[0];
    assert_eq!(
        resolve(SIMPLE, c, Resolution::Ours),
        "top\nours 1\nours 2\nbottom\n"
    );
    assert_eq!(
        resolve(SIMPLE, c, Resolution::Theirs),
        "top\ntheirs\nbottom\n"
    );
    assert_eq!(
        resolve(SIMPLE, c, Resolution::Both),
        "top\nours 1\nours 2\ntheirs\nbottom\n"
    );
    assert_eq!(resolve(SIMPLE, c, Resolution::Base), "top\nbottom\n");
}

#[test]
fn parses_diff3_with_crlf() {
    let text =
        "<<<<<<< ours\r\na\r\n||||||| base label\r\nb\r\nb2\r\n=======\r\nc\r\n>>>>>>> theirs\r\n";
    let conflicts = parse_conflicts(text);
    assert_eq!(conflicts.len(), 1);
    let c = &conflicts[0];
    assert_eq!(c.ours, 1..2);
    assert_eq!(c.base, Some(3..5));
    assert_eq!(c.base_label.as_deref(), Some("base label"));
    assert_eq!(c.theirs, 6..7);
    assert_eq!(c.ours_label, "ours");
    assert_eq!(resolve(text, c, Resolution::Base), "b\r\nb2\r\n");
    assert_eq!(resolve(text, c, Resolution::Theirs), "c\r\n");
}

#[test]
fn markers_need_exactly_seven_chars_at_line_start() {
    let text = "<<<<<<<< eight\n <<<<<<< indented\n<<<<<<<x\n=======\n>>>>>>>\n";
    assert!(parse_conflicts(text).is_empty());
    // `========` (eight) inside ours is content, not the separator.
    let text = "<<<<<<<\na\n========\n=======\nb\n>>>>>>>\n";
    let c = &parse_conflicts(text)[0];
    assert_eq!(c.ours, 1..3);
    assert_eq!(c.theirs, 4..5);
    assert_eq!(c.ours_label, "");
}

#[test]
fn nested_looking_content() {
    // A stray start marker restarts; separators and base markers in theirs
    // are content; an orphan end marker is ignored.
    let text = "\
>>>>>>> orphan
<<<<<<< stale
x
<<<<<<< HEAD
a
=======
b
=======
||||||| not base
>>>>>>> branch
<<<<<<< unterminated
y
";
    let conflicts = parse_conflicts(text);
    assert_eq!(conflicts.len(), 1);
    let c = &conflicts[0];
    assert_eq!(c.start_line, 3);
    assert_eq!(c.ours, 4..5);
    assert_eq!(c.theirs, 6..9);
    assert_eq!(c.end_line, 9);
    assert_eq!(c.theirs_label, "branch");
}

#[test]
fn end_without_separator_is_dropped() {
    assert!(parse_conflicts("<<<<<<< a\nx\n>>>>>>> b\n").is_empty());
}

#[test]
fn multiple_conflicts_resolve_last_first() {
    let text = "<<<<<<<\n1\n=======\n2\n>>>>>>>\nmid\n<<<<<<<\n3\n=======\n4\n>>>>>>>";
    let conflicts = parse_conflicts(text);
    assert_eq!(conflicts.len(), 2);
    let once = resolve(text, &conflicts[1], Resolution::Theirs);
    let twice = resolve(&once, &conflicts[0], Resolution::Ours);
    assert_eq!(twice, "1\nmid\n4\n");
}
