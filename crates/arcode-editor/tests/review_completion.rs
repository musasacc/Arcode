//! Adversarial review of the completion popup, snippets and signature
//! help. Every test here fails on the reviewed code and describes the
//! expected behaviour.

mod common;

use std::time::Duration;

use arcode_buffer::Point;
use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};
use arcode_editor::completion::COMPLETION_POPOVER;
use arcode_editor::{
    CompletionItem, CompletionList, CompletionTrigger, ParameterInfo, SignatureHelp, SignatureInfo,
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

/// An incomplete list that filters to nothing while its re-request is in
/// flight keeps an empty session: an empty popup box stays drawn and the
/// Up/Down keys are swallowed by it, so the cursor cannot move until the
/// (possibly slow) server answers.
#[gpui::test]
fn incomplete_list_filtered_to_nothing_swallows_arrow_keys(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "first line\n");
    let fake = Fake::new(|_, _, n| {
        // The first answer is instant, re-requests are slow.
        let delay = if n == 0 { 0 } else { 5_000 };
        (
            list(vec![lsp_item("foo_bar")], true),
            Duration::from_millis(delay),
        )
    });
    install(&editor, fake, cx);
    cx.simulate_input("f");
    assert!(labels(&editor, cx).contains(&"foo_bar".to_string()));
    // No item matches `fx`; the re-request is still running.
    cx.simulate_input("x");
    assert!(!visible(&editor, cx));
    let popover = editor.read_with(cx, |e, _| e.has_popover(COMPLETION_POPOVER));
    cx.simulate_keystrokes("up");
    let line = editor.read_with(cx, |e, _| e.cursor_position().line);
    assert_eq!(line, 0, "Up must move the cursor when no list is visible");
    assert!(
        !popover,
        "an empty completion popup box stays on screen while the re-request runs"
    );
}

/// `no_results_at` remembers only a char offset: after a word without
/// results is deleted with Backspace, a new word typed at the same place
/// never opens the list again.
#[gpui::test]
fn deleted_word_without_results_blocks_completion_at_same_offset(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let alpha_value = 1;\n");
    cx.simulate_input("zq");
    assert!(!visible(&editor, cx));
    cx.simulate_keystrokes("backspace backspace");
    assert!(text(&editor, cx).ends_with("= 1;\n"));
    cx.simulate_input("al");
    assert!(
        visible(&editor, cx),
        "typing `al` where a deleted no-result word was should offer alpha_value"
    );
    assert!(labels(&editor, cx).contains(&"alpha_value".to_string()));
}

/// After a trigger character only the server's items should show, but the
/// re-request of an incomplete list (rust-analyzer always answers
/// incomplete) is sent as `Incomplete`, which loses the "after a trigger
/// character" state: snippets are merged into the member list.
#[gpui::test]
fn member_list_after_trigger_gets_snippets_on_incomplete_rerequest(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {\n    v\n}");
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![Point::new(1, 5)..Point::new(1, 5)], cx)
        })
    });
    let fake = Fake::new(|_, _, _| {
        (
            list(vec![lsp_item("iter"), lsp_item("into_iter")], true),
            Duration::ZERO,
        )
    });
    install(&editor, fake, cx);
    cx.simulate_input(".");
    assert_eq!(labels(&editor, cx), vec!["into_iter", "iter"]);
    cx.simulate_input("i");
    let shown = labels(&editor, cx);
    assert!(
        shown.iter().all(|l| l == "iter" || l == "into_iter"),
        "after `v.` only members belong in the list, got {shown:?}"
    );
}

/// Same root cause: when the incomplete re-request after `.` returns no
/// items, buffer words fill the member list.
#[gpui::test]
fn member_list_after_trigger_gets_buffer_words_on_incomplete_rerequest(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = open(cx, "let item_word = 1;\nv");
    let fake = Fake::new(|_, _, n| {
        let items = if n == 0 {
            vec![lsp_item("iter")]
        } else {
            Vec::new()
        };
        (list(items, true), Duration::ZERO)
    });
    install(&editor, fake, cx);
    cx.simulate_input(".");
    assert_eq!(labels(&editor, cx), vec!["iter"]);
    cx.simulate_input("it");
    let shown = labels(&editor, cx);
    assert!(
        !shown.contains(&"item_word".to_string()),
        "buffer words must not appear after a trigger character, got {shown:?}"
    );
}

/// The ranker drops items with the same label and kind, so two server
/// items that differ only in what they import (e.g. `HashMap` from two
/// crates, `useState` from two packages) collapse into one and the user
/// cannot pick the other import.
#[gpui::test]
fn server_items_with_same_label_but_different_imports_are_all_shown(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, _| {
        let mut a = lsp_item("HashMap");
        a.candidate.kind = CandidateKind::Struct;
        a.candidate.detail = Some("std::collections::HashMap".into());
        a.additional_edits = vec![(
            Point::new(0, 0)..Point::new(0, 0),
            "use std::collections::HashMap;\n".into(),
        )];
        let mut b = lsp_item("HashMap");
        b.candidate.kind = CandidateKind::Struct;
        b.candidate.detail = Some("hashbrown::HashMap".into());
        b.additional_edits = vec![(
            Point::new(0, 0)..Point::new(0, 0),
            "use hashbrown::HashMap;\n".into(),
        )];
        (list(vec![a, b], false), Duration::ZERO)
    });
    install(&editor, fake, cx);
    cx.simulate_input("Hash");
    assert_eq!(
        labels(&editor, cx).len(),
        2,
        "both HashMap imports must be offered"
    );
}

fn add_signature() -> SignatureHelp {
    SignatureHelp {
        signatures: vec![SignatureInfo {
            label: "fn add(a: i32, b: i32) -> i32".into(),
            documentation: None,
            parameters: vec![ParameterInfo {
                label: 7..13,
                documentation: None,
            }],
            active_parameter: None,
        }],
        active_signature: 0,
        active_parameter: Some(0),
    }
}

/// `editor.completion.signature_help = false` should keep parameter hints
/// from opening automatically, but the `editor.action.triggerParameterHints`
/// command that rust-analyzer attaches to every function item opens them
/// on accept anyway.
#[gpui::test]
fn signature_setting_off_is_ignored_by_trigger_parameter_hints_command(
    cx: &mut TestAppContext,
) {
    let mut config = config();
    config.editor.completion.signature_help = false;
    let (editor, cx) = open_at(cx, "", "test.rs", config);
    let fake = Fake::new(|_, _, _| {
        let mut item = lsp_item("add");
        item.candidate.kind = CandidateKind::Function;
        item.candidate.insert = InsertAction::Snippet("add($0)".into());
        item.command = Some("editor.action.triggerParameterHints".into());
        (list(vec![item], false), Duration::ZERO)
    })
    .with_signature(|_, _| Some(add_signature()));
    install(&editor, fake, cx);
    cx.simulate_input("ad");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "add()");
    assert!(
        !editor.read_with(cx, |e, _| e.signature_help_visible()),
        "signature help is disabled in the settings"
    );
}

/// A plain-text item whose text contains CRLF (servers on Windows send
/// them) is inserted with LF by the buffer, but the cursor is placed by the
/// un-normalised char count, one char too far per line break.
#[gpui::test]
fn plain_text_item_with_crlf_places_cursor_at_its_end(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "\nzzz");
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![Point::new(0, 0)..Point::new(0, 0)], cx)
        })
    });
    let fake = Fake::new(|_, trigger, _| {
        assert_eq!(*trigger, CompletionTrigger::Invoked);
        let mut item = lsp_item("ab_two_lines");
        item.candidate.insert = InsertAction::PlainText("a\r\nb".into());
        (list(vec![item], false), Duration::ZERO)
    });
    install(&editor, fake, cx);
    cx.simulate_input("ab");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "a\nb\nzzz");
    let at = editor.read_with(cx, |e, _| e.cursor_position());
    assert_eq!(at, Point::new(1, 1), "cursor right after the inserted text");
}

/// With an incomplete list (rust-analyzer always answers incomplete), the
/// answer to the re-request sent for the last keystroke replaces the list
/// and resets the selection to the first row, even after the user moved
/// it with the arrows. Pressing Enter just after that accepts an item the
/// user did not choose.
#[gpui::test]
fn late_incomplete_answer_keeps_the_row_the_user_selected(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let fake = Fake::new(|_, _, n| {
        let items = vec![lsp_item("alpha"), lsp_item("alpine"), lsp_item("alps")];
        let delay = if n == 0 { 0 } else { 100 };
        (list(items, true), Duration::from_millis(delay))
    });
    install(&editor, fake, cx);
    cx.simulate_input("a");
    // The re-request for `l` is in flight while the user picks a row.
    cx.simulate_input("l");
    cx.simulate_keystrokes("down");
    let chosen = editor.read_with(cx, |e, _| {
        e.selected_completion().map(|i| i.candidate.label.clone())
    });
    assert!(chosen.is_some());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    let now = editor.read_with(cx, |e, _| {
        e.selected_completion().map(|i| i.candidate.label.clone())
    });
    assert_eq!(now, chosen, "the answer must not move the user's selection");
}

/// A provider whose resolve answers after 100 ms and counts the calls.
struct SlowResolve {
    resolves: std::rc::Rc<std::cell::Cell<usize>>,
}

impl arcode_editor::CompletionProvider for SlowResolve {
    fn completions(
        &self,
        _: &arcode_editor::Editor,
        _: &CompletionTrigger,
        _: &mut gpui::Context<arcode_editor::Editor>,
    ) -> Option<gpui::Task<anyhow::Result<CompletionList>>> {
        let mut a = lsp_item("alpha_one");
        a.needs_resolve = true;
        let mut b = lsp_item("beta_two");
        b.needs_resolve = true;
        Some(gpui::Task::ready(Ok(list(vec![a, b], false))))
    }

    fn resolve(
        &self,
        _: &arcode_editor::Editor,
        item: &CompletionItem,
        cx: &mut gpui::Context<arcode_editor::Editor>,
    ) -> Option<gpui::Task<anyhow::Result<CompletionItem>>> {
        self.resolves.set(self.resolves.get() + 1);
        let item = item.clone();
        Some(cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            Ok(item)
        }))
    }
}

/// While the selected item's resolve is in flight, every keystroke that
/// re-filters the list drops it and sends another `completionItem/resolve`
/// for the same item, so a server gets one request per keystroke and the
/// docs only appear once the user pauses.
#[gpui::test]
fn typing_does_not_resend_resolve_for_the_same_selected_item(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let resolves = std::rc::Rc::new(std::cell::Cell::new(0));
    let provider: std::rc::Rc<dyn arcode_editor::CompletionProvider> =
        std::rc::Rc::new(SlowResolve {
            resolves: resolves.clone(),
        });
    editor.update(cx, |e, _| e.set_completion_provider(Some(provider)));
    cx.simulate_input("a");
    cx.simulate_input("l");
    cx.simulate_input("p");
    cx.simulate_input("h");
    assert_eq!(labels(&editor, cx), vec!["alpha_one"]);
    assert_eq!(resolves.get(), 1, "one resolve for the one selected item");
}
