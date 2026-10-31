//! Review tests (plugins part 2: consent dialog). Each failing test
//! reproduces a defect found in review.

use std::time::Duration;

use arcode_plugin_host::PluginId;
use arcode_plugin_host::cancel::{CancelToken, ReplyReceiver, oneshot};
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ConsentRequest};
use arcode_plugin_host::grants::GrantScope;
use gpui::{PromptLevel, TestAppContext};

use super::tests::open;
use crate::plugins::consent::ConsentAsk;

fn ask() -> (ConsentAsk, ReplyReceiver<ConsentAnswer>) {
    let (reply, answer) = oneshot();
    let request = ConsentRequest {
        plugin: PluginId::parse("acme.tools").expect("id"),
        plugin_name: "\u{201c}Acme Tools\u{201d}".into(),
        project: None,
        kind: ConsentKind::Network {
            method: "GET".into(),
            scheme: "https".into(),
            host: "example.com".into(),
            port: 443,
        },
    };
    let ask = ConsentAsk {
        request,
        cancel: CancelToken::new(),
        reply,
    };
    (ask, answer)
}

/// Another window prompt (an unsaved-changes question, the uninstall or
/// enable confirmation, a disk-conflict prompt...) replaces the consent
/// dialog in GPUI's single prompt slot. The consent dialog entity is still
/// held by the workspace task, so its `closed` future never resolves:
/// `prompts_open` stays 1 for good, every later consent request is queued
/// and never shown, and the waiting plugin thread gets no answer.
#[gpui::test]
fn review_an_app_prompt_over_the_consent_dialog_wedges_consent(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    // The app's own themed prompts, as on Linux.
    cx.update(|_, cx| crate::prompt::init_themed(cx));
    let (first, first_answer) = ask();
    ws.update_in(cx, |ws, window, cx| ws.plugin_ask(first, window, cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert_eq!(ws.plugins.prompts_open, 1));

    // The app asks something while the consent dialog is open; the user
    // answers it.
    let app = ws.update_in(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Warning,
            "Save changes?",
            None,
            &["Save", "Cancel"],
            cx,
        )
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    drop(app);

    // The replaced consent request is over (dismissed) ...
    let first = first_answer.wait(&CancelToken::new(), Some(Duration::from_millis(300)));
    let open_after = ws.read_with(cx, |ws, _| ws.plugins.prompts_open);

    // ... and the next request is shown and can be answered.
    let (second, second_answer) = ask();
    ws.update_in(cx, |ws, window, cx| ws.plugin_ask(second, window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let second = second_answer.wait(&CancelToken::new(), Some(Duration::from_millis(300)));
    let queued = ws.read_with(cx, |ws, _| ws.plugins.consent_queue.len());
    eprintln!("first={first:?} prompts_open={open_after} second={second:?} queued={queued}");
    assert_eq!(
        first.ok(),
        Some(ConsentAnswer::Dismissed),
        "the plugin thread still waits for the replaced dialog"
    );
    assert_eq!(open_after, 0, "the replaced dialog is still counted");
    assert_eq!(
        second.ok(),
        Some(ConsentAnswer::Allow {
            scope: GrantScope::Once,
            write: false
        }),
        "a later consent request is never shown"
    );
}

/// The dialog appears while a key is down (Enter held after Ctrl+Enter
/// "Send", or Enter pressed while typing when an activation asks for file
/// access). Key repeats (`is_held`) keep arriving after the arm delay and
/// press the focused default button, "Allow once": access is granted
/// without the user ever choosing it.
#[gpui::test]
fn review_a_held_key_answers_the_consent_dialog(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let (first, answer) = ask();
    ws.update_in(cx, |ws, window, cx| ws.plugin_ask(first, window, cx));
    cx.run_until_parked();
    // An auto-repeat of the Enter key that was already down.
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("enter").expect("keystroke"),
        is_held: true,
    });
    cx.run_until_parked();
    let answer = answer.wait(&CancelToken::new(), Some(Duration::from_millis(100)));
    assert!(
        answer.is_err(),
        "a key repeat answered the consent dialog: {answer:?}"
    );
}
