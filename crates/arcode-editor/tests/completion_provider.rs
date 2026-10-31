//! Completion and signature help from a provider (as the language server
//! bridge supplies them): edits, additional edits, stale answers,
//! incomplete lists, commit characters, resolving and signature help.

mod common;

use std::time::Duration;

use arcode_buffer::Point;
use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};
use arcode_editor::{
    CompletionItem, CompletionList, CompletionTrigger, Documentation, ItemEdit, ParameterInfo,
    SignatureHelp, SignatureInfo,
};
use common::*;
use gpui::TestAppContext;

fn lsp_item(label: &str) -> CompletionItem {
    let candidate = CompletionCandidate::new(
        label,
        CandidateKind::Method,
        CandidateSource::Lsp {
            server: "fake".into(),
        },
    );
    CompletionItem::new(candidate)
}

fn list(items: Vec<CompletionItem>, is_incomplete: bool) -> CompletionList {
    CompletionList {
        items,
        is_incomplete,
    }
}

#[gpui::test]
fn text_edits_and_additional_edits_apply_as_one_step(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {\n    let v = Vec::new();\n    v\n}");
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![Point::new(2, 5)..Point::new(2, 5)], cx)
        })
    });
    let fake = Fake::new(|e, trigger, _| {
        assert_eq!(*trigger, CompletionTrigger::Character(".".into()));
        let at = e.cursor_position();
        let mut push = lsp_item("push");
        push.candidate.insert = InsertAction::Snippet("push(${1:value})$0".into());
        push.edit = Some(ItemEdit::new(at..at));
        push.additional_edits = vec![(
            Point::new(0, 0)..Point::new(0, 0),
            "use std::vec::Vec;\n".into(),
        )];
        (
            list(vec![push, lsp_item("pop"), lsp_item("len")], false),
            Duration::ZERO,
        )
    });
    install(&editor, fake, cx);
    cx.simulate_input(".");
    assert_eq!(labels(&editor, cx), vec!["len", "pop", "push"]);
    // Typing filters the list; the typed text is replaced on accept.
    cx.simulate_input("pu");
    assert_eq!(labels(&editor, cx), vec!["push"]);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        text(&editor, cx),
        "use std::vec::Vec;\nfn main() {\n    let v = Vec::new();\n    v.push(value)\n}"
    );
    // The placeholder is selected, at the shifted position.
    let sel = editor.read_with(cx, |e, _| e.selections().primary().range());
    let sel_text = editor.read_with(cx, |e, _| e.buffer().slice_to_string(sel));
    assert_eq!(sel_text, "value");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(
        text(&editor, cx),
        "fn main() {\n    let v = Vec::new();\n    v.pu\n}"
    );
}

#[gpui::test]
fn stale_answers_are_dropped(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    // The first request answers late, the second one early.
    let fake = Fake::new(|_, _, n| {
        let (label, delay) = if n == 0 {
            ("stale_one", 300)
        } else {
            ("fresh_one", 10)
        };
        (
            list(vec![lsp_item(label)], false),
            Duration::from_millis(delay),
        )
    });
    install(&editor, fake, cx);
    cx.simulate_input("s");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("ctrl-space");
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();
    let shown = labels(&editor, cx);
    assert!(shown.contains(&"fresh_one".to_string()), "{shown:?}");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(
        labels(&editor, cx),
        shown,
        "the late first answer changes nothing"
    );
    assert!(!shown.contains(&"stale_one".to_string()));
}

#[gpui::test]
fn incomplete_lists_are_asked_again_and_words_fill_in(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let local_word = 1;\n");
    let fake = Fake::new(|e, _, _| {
        // The server answers for what was typed so far.
        let line = e.buffer().line_text(1);
        let items = if line == "l" {
            Vec::new()
        } else {
            vec![lsp_item(&format!("{line}_from_server"))]
        };
        (list(items, true), Duration::ZERO)
    });
    let calls = fake.calls.clone();
    install(&editor, fake, cx);
    cx.simulate_input("l");
    // The server had nothing: buffer words fill in.
    assert!(labels(&editor, cx).contains(&"local_word".to_string()));
    cx.simulate_input("o");
    assert_eq!(labels(&editor, cx)[0], "lo_from_server");
    cx.simulate_input("c");
    assert_eq!(labels(&editor, cx)[0], "loc_from_server");
    assert_eq!(
        *calls.borrow(),
        vec![
            CompletionTrigger::Invoked,
            CompletionTrigger::Incomplete,
            CompletionTrigger::Incomplete
        ]
    );
}

#[gpui::test]
fn complete_lists_only_filter_while_typing(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, _| {
        let items = vec![lsp_item("format"), lsp_item("from_str")];
        (list(items, false), Duration::ZERO)
    });
    let calls = fake.calls.clone();
    install(&editor, fake, cx);
    cx.simulate_input("f");
    cx.simulate_input("s");
    assert_eq!(labels(&editor, cx), vec!["from_str"]);
    assert_eq!(calls.borrow().len(), 1);
    // A word with no results is not asked again while typing on.
    cx.simulate_input("zz");
    assert!(!visible(&editor, cx));
    assert_eq!(
        calls.borrow().len(),
        1,
        "closed lists are not re-requested mid-word"
    );
}

#[gpui::test]
fn commit_characters_accept_then_type(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, _| {
        let mut item = lsp_item("println");
        item.commit_characters = vec!["!".into()];
        (list(vec![item], false), Duration::ZERO)
    });
    install(&editor, fake, cx);
    cx.simulate_input("pri");
    assert!(visible(&editor, cx));
    cx.simulate_input("!");
    assert_eq!(text(&editor, cx), "println!");
    assert!(!visible(&editor, cx));
}

#[gpui::test]
fn the_selected_item_is_resolved_for_docs(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, _| {
        let mut a = lsp_item("alpha");
        a.needs_resolve = true;
        let mut b = lsp_item("alps");
        b.needs_resolve = true;
        (list(vec![a, b], false), Duration::ZERO)
    })
    .with_resolve(|item| {
        let mut item = item.clone();
        item.documentation = Some(Documentation::Markdown(format!(
            "**{}** docs",
            item.candidate.label
        )));
        item.candidate.detail = Some("fn()".into());
        item
    });
    let resolves = fake.resolves.clone();
    install(&editor, fake, cx);
    cx.simulate_input("al");
    let docs = |cx: &mut gpui::VisualTestContext| {
        editor.read_with(cx, |e, _| {
            e.selected_completion()
                .and_then(|i| i.documentation.clone())
        })
    };
    assert_eq!(
        docs(cx),
        Some(Documentation::Markdown("**alpha** docs".into()))
    );
    cx.simulate_keystrokes("down");
    assert_eq!(
        docs(cx),
        Some(Documentation::Markdown("**alps** docs".into()))
    );
    cx.simulate_keystrokes("up");
    assert_eq!(*resolves.borrow(), 2, "resolved once per item");
}

fn add_signature(active: Option<usize>) -> SignatureHelp {
    SignatureHelp {
        signatures: vec![SignatureInfo {
            label: "fn add(a: i32, b: i32) -> i32".into(),
            documentation: Some(Documentation::PlainText("Adds.".into())),
            parameters: vec![
                ParameterInfo {
                    label: 7..13,
                    documentation: None,
                },
                ParameterInfo {
                    label: 15..21,
                    documentation: None,
                },
            ],
            active_parameter: None,
        }],
        active_signature: 0,
        active_parameter: active,
    }
}

#[gpui::test]
fn signature_help_follows_the_call(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake =
        Fake::new(|_, _, _| (list(Vec::new(), false), Duration::ZERO)).with_signature(|e, _| {
            let line = e.buffer().line_text(0);
            let col = e.cursor_position().column;
            let before: String = line.chars().take(col).collect();
            let open = before.rfind('(')?;
            if before[open..].contains(')') {
                return None;
            }
            Some(add_signature(Some(before[open..].matches(',').count())))
        });
    install(&editor, fake, cx);
    let active = |cx: &mut gpui::VisualTestContext| {
        editor.read_with(cx, |e, _| {
            e.signature_help().and_then(|h| h.active_parameter)
        })
    };
    cx.simulate_input("add(");
    assert_eq!(active(cx), Some(0));
    cx.simulate_input("1, ");
    assert_eq!(active(cx), Some(1));
    // Leaving the call (past the auto-closed paren) closes it.
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.signature_help_visible()));
    cx.simulate_keystrokes("left ctrl-shift-space");
    cx.run_until_parked();
    assert_eq!(active(cx), Some(1));
    cx.simulate_keystrokes("escape");
    assert!(!editor.read_with(cx, |e, _| e.signature_help_visible()));
}

#[gpui::test]
fn signature_setting_off_keeps_it_closed(cx: &mut TestAppContext) {
    let mut config = config();
    config.editor.completion.signature_help = false;
    let (editor, cx) = open_at(cx, "", "test.rs", config);
    let fake = Fake::new(|_, _, _| (list(Vec::new(), false), Duration::ZERO))
        .with_signature(|_, _| Some(add_signature(Some(0))));
    install(&editor, fake, cx);
    cx.simulate_input("add(");
    assert!(!editor.read_with(cx, |e, _| e.signature_help_visible()));
}

#[gpui::test]
fn trigger_characters_without_server_items_show_nothing(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let words_here = 1;\n");
    let mut fake = Fake::new(|_, _, _| (list(Vec::new(), false), Duration::ZERO));
    fake.triggers = vec!["(".into()];
    let calls = fake.calls.clone();
    install(&editor, fake, cx);
    cx.simulate_input("f(");
    assert_eq!(
        calls.borrow().last(),
        Some(&CompletionTrigger::Character("(".into()))
    );
    assert!(!visible(&editor, cx), "{:?}", labels(&editor, cx));
}
