//! Regression tests for the review fixes of the completion popup that the
//! review tests do not cover.

mod common;

use std::time::Duration;

use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate};
use arcode_editor::CompletionItem;
use arcode_editor::CompletionList;
use arcode_editor::completion::COMPLETION_POPOVER;
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

fn selected(
    editor: &gpui::Entity<arcode_editor::Editor>,
    cx: &mut gpui::VisualTestContext,
) -> Option<String> {
    editor.read_with(cx, |e, _| {
        e.selected_completion().map(|i| i.candidate.label.clone())
    })
}

/// Shift+Tab back into a choice placeholder selects the value it holds.
#[gpui::test]
fn reentered_choice_list_selects_the_current_value(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let toml = "[[snippet]]\nprefix = \"logx\"\nbody = \"log::${1|debug,info,warn|}!(\\\"${2:msg}\\\");$0\"\nlanguages = [\"rust\"]\n";
    let loaded = cx.update(|_, cx| arcode_editor::completion::load_user_snippets(toml, cx));
    assert_eq!(loaded.unwrap(), 1);
    cx.simulate_input("logx");
    cx.simulate_keystrokes("enter");
    assert_eq!(selected(&editor, cx).as_deref(), Some("debug"));
    cx.simulate_keystrokes("down enter");
    assert_eq!(text(&editor, cx), "log::info!(\"msg\");");
    cx.simulate_keystrokes("tab shift-tab");
    assert_eq!(labels(&editor, cx), vec!["debug", "info", "warn"]);
    assert_eq!(selected(&editor, cx).as_deref(), Some("info"));
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "log::info!(\"msg\");");
}

/// An incomplete member list whose answer is empty stays (hidden), so the
/// next keystroke asks again as a member list: no words or snippets, and
/// the box comes back when the server has items again.
#[gpui::test]
fn empty_incomplete_member_answer_keeps_the_member_list(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let item_word = 1;\nv");
    let fake = Fake::new(|e, _, _| {
        let typed = e.buffer().to_string();
        let items = if typed.ends_with("v.itx") {
            Vec::new()
        } else {
            vec![lsp_item("itx_member"), lsp_item("iter")]
        };
        (
            CompletionList {
                items,
                is_incomplete: true,
            },
            Duration::ZERO,
        )
    });
    let calls = fake.calls.clone();
    install(&editor, fake, cx);
    cx.simulate_input(".");
    assert_eq!(labels(&editor, cx), vec!["iter", "itx_member"]);
    cx.simulate_input("itx");
    assert!(!visible(&editor, cx));
    assert!(!editor.read_with(cx, |e, _| e.has_popover(COMPLETION_POPOVER)));
    cx.simulate_input("_");
    let shown = labels(&editor, cx);
    assert_eq!(shown, vec!["itx_member"], "only members, got {shown:?}");
    assert!(editor.read_with(cx, |e, _| e.has_popover(COMPLETION_POPOVER)));
    let calls = calls.borrow();
    assert!(
        calls[1..]
            .iter()
            .all(|t| *t == arcode_editor::CompletionTrigger::Incomplete),
        "re-requests stay incomplete requests: {calls:?}"
    );
}

/// While an empty list waits for its re-request, Enter types a newline.
#[gpui::test]
fn enter_is_typed_while_the_list_is_empty(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, n| {
        let delay = if n == 0 { 0 } else { 5_000 };
        (
            CompletionList {
                items: vec![lsp_item("foo_bar")],
                is_incomplete: true,
            },
            Duration::from_millis(delay),
        )
    });
    install(&editor, fake, cx);
    cx.simulate_input("f");
    assert!(visible(&editor, cx));
    cx.simulate_input("x");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "fx\n");
}

/// Accepting a function whose snippet selects its first argument opens
/// parameter hints through the `triggerParameterHints` command.
#[gpui::test]
fn parameter_hints_open_with_a_selected_placeholder(cx: &mut TestAppContext) {
    use arcode_completion::InsertAction;
    use arcode_editor::{ParameterInfo, SignatureHelp, SignatureInfo};
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, _| {
        let mut item = lsp_item("add");
        item.candidate.kind = CandidateKind::Function;
        item.candidate.insert = InsertAction::Snippet("add(${1:a})$0".into());
        item.command = Some("editor.action.triggerParameterHints".into());
        (
            CompletionList {
                items: vec![item],
                is_incomplete: false,
            },
            Duration::ZERO,
        )
    })
    .with_signature(|_, _| {
        Some(SignatureHelp {
            signatures: vec![SignatureInfo {
                label: "fn add(a: i32) -> i32".into(),
                documentation: None,
                parameters: vec![ParameterInfo {
                    label: 7..13,
                    documentation: None,
                }],
                active_parameter: None,
            }],
            active_signature: 0,
            active_parameter: Some(0),
        })
    });
    install(&editor, fake, cx);
    cx.simulate_input("ad");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "add(a)");
    assert!(editor.read_with(cx, |e, _| e.signature_help_visible()));
}

/// A request scheduled while typing a word is dropped when the cursor left
/// that word before the delay ended: typing `u8 = "q";` fast must not ask
/// for (and show) every item in scope after the `;`, where Enter would
/// accept one instead of breaking the line.
#[gpui::test]
fn request_scheduled_for_a_word_is_dropped_after_fast_typing(cx: &mut TestAppContext) {
    let mut config = config();
    config.editor.completion.delay_ms = 50;
    let (editor, cx) = open_at(cx, "", "test.rs", config);
    let fake = Fake::new(|_, _, _| {
        let items = ["None", "Some", "u8"].map(lsp_item).to_vec();
        (
            CompletionList {
                items,
                is_incomplete: false,
            },
            Duration::ZERO,
        )
    });
    let calls = fake.calls.clone();
    install(&editor, fake, cx);
    for c in ["l", "e", "t", " ", "z", ":", " ", "u", "8", ";"] {
        cx.simulate_input(c);
    }
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert!(calls.borrow().is_empty(), "sent {:?}", calls.borrow());
    assert!(!visible(&editor, cx));
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "let z: u8;\n");
    // Pausing inside a word still asks.
    cx.simulate_input("S");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(labels(&editor, cx), vec!["Some"]);
}
