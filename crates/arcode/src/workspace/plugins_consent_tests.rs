//! The consent dialog and the app's own prompts share GPUI's one prompt
//! slot per window: a consent request waits for an open app prompt, and
//! one replaced by an app prompt is dismissed, the next request comes
//! after the app prompt and the focus goes back where it was.

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
        plugin_name: "Acme Tools".into(),
        project: None,
        kind: ConsentKind::Network {
            method: "GET".into(),
            scheme: "https".into(),
            host: "example.com".into(),
            port: 443,
        },
    };
    let cancel = CancelToken::new();
    (
        ConsentAsk {
            request,
            cancel,
            reply,
        },
        answer,
    )
}

const SHORT: Option<Duration> = Some(Duration::from_millis(200));

fn once() -> Option<ConsentAnswer> {
    Some(ConsentAnswer::Allow {
        scope: GrantScope::Once,
        write: false,
    })
}

#[gpui::test]
fn a_request_waits_for_an_open_app_prompt(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.update(|_, cx| crate::prompt::init_themed(cx));
    let app = ws.update_in(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Info,
            "Reload?",
            None,
            &["Reload", "Cancel"],
            cx,
        )
    });
    cx.run_until_parked();
    let (request, answer) = ask();
    ws.update_in(cx, |ws, window, cx| ws.plugin_ask(request, window, cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.prompts_open, 0, "the app prompt stays");
        assert_eq!(ws.plugins.consent_queue.len(), 1);
    });
    // Enter answers the app prompt, not the waiting consent request.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    drop(app);
    ws.read_with(cx, |ws, _| assert_eq!(ws.plugins.prompts_open, 1));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(answer.wait(&CancelToken::new(), SHORT).ok(), once());
}

#[gpui::test]
fn after_a_replaced_dialog_the_next_request_follows_the_app_prompt(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.update(|_, cx| crate::prompt::init_themed(cx));
    let focus = ws.read_with(cx, |ws, _| ws.focus_handle.clone());
    let (first, first_answer) = ask();
    let (second, second_answer) = ask();
    ws.update_in(cx, |ws, window, cx| {
        ws.plugin_ask(first, window, cx);
        ws.plugin_ask(second, window, cx);
    });
    cx.run_until_parked();
    let app = ws.update_in(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Warning,
            "Close?",
            None,
            &["Close", "Cancel"],
            cx,
        )
    });
    cx.run_until_parked();
    let first = first_answer.wait(&CancelToken::new(), SHORT);
    assert_eq!(first.ok(), Some(ConsentAnswer::Dismissed));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.plugins.prompts_open, 0);
        assert_eq!(
            ws.plugins.consent_queue.len(),
            1,
            "waits for the app prompt"
        );
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    drop(app);
    ws.read_with(cx, |ws, _| assert_eq!(ws.plugins.prompts_open, 1));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(second_answer.wait(&CancelToken::new(), SHORT).ok(), once());
    cx.update(|window, _| {
        assert!(
            focus.is_focused(window),
            "the focus is back on the workspace"
        )
    });
}
