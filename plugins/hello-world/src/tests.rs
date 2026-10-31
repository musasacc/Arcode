//! The plugin end to end against the SDK's fake host.
//!
//! On native targets every host function (`host::notify`, `editor::text`,
//! `kv::set`, `Panel::open`, ...) goes to `arcode_plugin::testing`, a fake
//! host whose state you arrange and inspect with `testing::with`. Each
//! test thread has its own fake; `testing::reset()` starts from scratch.

use arcode_plugin::editor::{Position, Range};
use arcode_plugin::testing::{self, FakePanel};
use arcode_plugin::ui::{Event, EventKind, Widget};
use arcode_plugin::{Json, Plugin, host, kv};

use crate::{HelloWorld, PANEL, TodoArgs, view};

const SOURCE: &str = "\
fn main() {
    // TODO: print a greeting
    println!(\"hi\");
}
";

/// A fresh fake host with one Rust document open and focused.
fn setup() -> (HelloWorld, u64) {
    testing::reset();
    let doc = testing::with(|h| {
        let doc = h.add_document(Some("src/main.rs"), "rust", SOURCE);
        h.current_document = Some(doc);
        doc
    });
    (HelloWorld::default(), doc)
}

/// Every text shown in a fake panel.
fn texts(panel: &FakePanel) -> Vec<String> {
    panel
        .view
        .nodes
        .values()
        .filter_map(|node| match &node.widget {
            Widget::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn say_hello_notifies_with_the_configured_greeting() {
    let (mut plugin, _) = setup();
    testing::with(|h| h.settings = r#"{"greeting": "Hi"}"#.to_owned());
    plugin.command("say-hello", Json::null()).unwrap();
    let shown = testing::with(|h| h.notifications.clone());
    assert_eq!(
        shown,
        [(
            host::Level::Info,
            "Hi from a plugin! You are editing src/main.rs.".to_owned()
        )]
    );
}

#[test]
fn lenses_sit_on_todo_lines_and_carry_their_text() {
    let (mut plugin, doc) = setup();
    let info = testing::with(|h| h.documents[&doc].info.clone());
    let lenses = plugin.code_lenses(&info);
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].line, 1);
    assert_eq!(lenses[0].command, "explain-todo");
    let args: TodoArgs = Json::from(lenses[0].args.clone()).parse().unwrap();
    assert_eq!(args.text, "TODO: print a greeting");
}

#[test]
fn explain_todo_uses_lens_args_or_the_cursor_line() {
    let (mut plugin, doc) = setup();
    let args = Json::from_value(&TodoArgs {
        line: 1,
        text: "TODO: x".into(),
    });
    plugin.command("explain-todo", args).unwrap();

    // From the palette there are no args; the cursor is on line 1.
    testing::with(|h| {
        let at = Position { line: 1, column: 4 };
        h.documents.get_mut(&doc).unwrap().selections = vec![Range { start: at, end: at }];
    });
    plugin.command("explain-todo", Json::null()).unwrap();

    let shown: Vec<String> =
        testing::with(|h| h.notifications.iter().map(|(_, m)| m.clone()).collect());
    assert_eq!(shown, ["Line 2: TODO: x", "Line 2: TODO: print a greeting"]);
}

#[test]
fn greet_button_updates_the_panel_and_keeps_the_count() {
    let (mut plugin, _) = setup();
    plugin.command("show-panel", Json::null()).unwrap();
    let panel_id = testing::with(|h| h.panels.keys().next().copied()).unwrap();
    assert!(
        testing::with(|h| texts(h.panel(PANEL).unwrap()))
            .contains(&"Greeted 0 times so far.".to_owned())
    );

    // The user types a name and clicks Greet: the click event carries the
    // input's text in `values`.
    let p = plugin.panel.as_ref().unwrap();
    let (input, greet) = (
        p.node_id(view::NAME_INPUT).unwrap(),
        p.node_id(view::GREET_BUTTON).unwrap(),
    );
    let click = Event {
        node: greet,
        kind: EventKind::Click,
        values: vec![(input, "Ada".to_owned())],
    };
    plugin.ui_event(panel_id, click).unwrap();

    let shown = testing::with(|h| texts(h.panel(PANEL).unwrap()));
    assert!(shown.contains(&"Hello, Ada!".to_owned()), "{shown:?}");
    assert!(
        shown.contains(&"Greeted 1 time so far.".to_owned()),
        "{shown:?}"
    );
    // The second draw was a patch, not a full render.
    assert_eq!(testing::with(|h| h.panel(PANEL).unwrap().renders), 1);
    assert_eq!(
        kv::get(kv::Scope::Global, "greet-clicks"),
        Some(b"1".to_vec())
    );
}

#[test]
fn unknown_commands_are_errors() {
    let (mut plugin, _) = setup();
    let err = plugin.command("nope", Json::null()).unwrap_err();
    assert_eq!(err.message(), "unknown command `nope`");
}
