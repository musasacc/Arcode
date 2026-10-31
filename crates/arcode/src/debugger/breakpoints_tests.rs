use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn toggle_adds_and_removes() {
    let mut book = BreakpointBook::default();
    assert!(book.toggle(&p("/a.rs"), 3));
    assert!(book.at(&p("/a.rs"), 3).is_some());
    assert!(!book.toggle(&p("/a.rs"), 3));
    assert!(book.is_empty());
}

#[test]
fn only_enabled_breakpoints_are_sent_sorted_and_one_based() {
    let mut book = BreakpointBook::default();
    let b = book.add(&p("/a.rs"), 9);
    let a = book.add(&p("/a.rs"), 2);
    let off = book.add(&p("/a.rs"), 5);
    book.set_enabled(off, false);
    book.set_options(&p("/a.rs"), 9, Some("x > 1".into()), Some(" ".into()), None);
    let (ids, list) = book.source_breakpoints(&p("/a.rs"));
    assert_eq!(ids, [a, b]);
    assert_eq!(list[0].line, 3);
    assert_eq!(list[1].line, 10);
    assert_eq!(list[1].condition.as_deref(), Some("x > 1"));
    assert_eq!(list[1].hit_condition, None);
    assert_eq!(book.get(b).unwrap().kind(), BpKind::Conditional);
    assert_eq!(book.all_source_breakpoints().len(), 1);
}

#[test]
fn relocation_follows_the_editor_and_drops_deleted_lines() {
    let mut book = BreakpointBook::default();
    let a = book.add(&p("/a.rs"), 2);
    let b = book.add(&p("/a.rs"), 6);
    let other = book.add(&p("/b.rs"), 6);
    assert!(book.relocate(&p("/a.rs"), &[(b, 4)]));
    assert!(book.get(a).is_none());
    assert_eq!(book.get(b).unwrap().line, 4);
    assert_eq!(book.get(other).unwrap().line, 6);
    assert!(!book.relocate(&p("/a.rs"), &[(b, 4)]));
}

#[test]
fn verification_is_applied_in_send_order() {
    let mut book = BreakpointBook::default();
    let a = book.add(&p("/a.rs"), 1);
    let answer = vec![DapBreakpoint {
        verified: false,
        message: Some("no code".into()),
        ..Default::default()
    }];
    book.apply_verification(&[a], &answer);
    assert_eq!(book.get(a).unwrap().verified, Some(false));
    book.clear_verification();
    assert_eq!(book.get(a).unwrap().verified, None);
}

#[test]
fn saved_round_trip_and_renames() {
    let mut book = BreakpointBook::default();
    book.set_options(&p("/x/a.rs"), 1, None, None, Some("v={v}".into()));
    let off = book.add(&p("/x/b.rs"), 2);
    book.set_enabled(off, false);
    let saved = book.to_saved();
    let back = BreakpointBook::from_saved(&saved);
    assert_eq!(back.to_saved(), saved);
    assert_eq!(back.at(&p("/x/a.rs"), 1).unwrap().kind(), BpKind::LogPoint);
    let mut book = back;
    assert!(book.rename(&p("/x"), &p("/y")));
    assert!(book.at(&p("/y/b.rs"), 2).is_some());
    assert_eq!(book.clear().len(), 2);
}
