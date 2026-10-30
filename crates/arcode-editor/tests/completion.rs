//! Completion from local sources (buffer words, paths, snippets), keys and
//! settings, through GPUI's test platform.

mod common;

use arcode_buffer::{Buffer, Point};
use arcode_editor::Editor;
use common::*;
use gpui::{AppContext, TestAppContext};

#[gpui::test]
fn buffer_words_complete_and_enter_accepts(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let alpha_beta = 1;\nlet alpine = 2;\n");
    cx.simulate_input("al");
    assert!(visible(&editor, cx));
    let shown = labels(&editor, cx);
    assert!(shown.contains(&"alpha_beta".to_string()), "{shown:?}");
    assert!(shown.contains(&"alpine".to_string()), "{shown:?}");
    // Narrowing the query re-filters without asking again.
    cx.simulate_input("pi");
    assert_eq!(labels(&editor, cx), vec!["alpine"]);
    cx.simulate_keystrokes("enter");
    assert!(!visible(&editor, cx));
    assert!(text(&editor, cx).ends_with("let alpine = 2;\nalpine"));
    // Accepting is one undo step back to the typed text.
    cx.simulate_keystrokes("secondary-z");
    assert!(
        text(&editor, cx).ends_with("\nalpi"),
        "{:?}",
        text(&editor, cx)
    );
}

#[gpui::test]
fn navigation_escape_and_leaving_the_word_close(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "value_one value_two value_three\n");
    cx.simulate_input("va");
    let first = labels(&editor, cx);
    assert_eq!(first.len(), 3, "{first:?}");
    cx.simulate_keystrokes("down down");
    let selected = editor.read_with(cx, |e, _| {
        e.selected_completion().map(|i| i.candidate.label.clone())
    });
    assert_eq!(selected.as_deref(), Some(first[2].as_str()));
    cx.simulate_keystrokes("up up up");
    let wrapped = editor.read_with(cx, |e, _| {
        e.selected_completion().map(|i| i.candidate.label.clone())
    });
    assert_eq!(wrapped.as_deref(), Some(first[2].as_str()), "wraps around");
    cx.simulate_keystrokes("escape");
    assert!(!visible(&editor, cx));
    assert!(
        text(&editor, cx).ends_with("\nva"),
        "escape inserts nothing"
    );
    // Typing on opens it again; a space (not a word char) closes it.
    cx.simulate_input("l");
    assert!(visible(&editor, cx));
    cx.simulate_input(" ");
    assert!(!visible(&editor, cx));
    // Moving the cursor out of the word closes it too.
    cx.simulate_input("va");
    assert!(visible(&editor, cx));
    cx.simulate_keystrokes("left left");
    assert!(visible(&editor, cx), "still at the start of the word");
    cx.simulate_keystrokes("left");
    assert!(!visible(&editor, cx));
}

#[gpui::test]
fn words_come_from_every_open_buffer(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {}\n");
    let other =
        cx.update(|_, cx| cx.new(|cx| Editor::new(Buffer::new("let zebra_count = 3;\n"), cx)));
    cx.simulate_input("zeb");
    assert_eq!(labels(&editor, cx), vec!["zebra_count"]);
    cx.simulate_keystrokes("tab");
    assert!(text(&editor, cx).ends_with("zebra_count"));
    drop(other);
}

#[gpui::test]
fn shift_enter_replaces_the_rest_of_the_word(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "counter_total\ncounter_tail\n");
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![Point::new(1, 8)..Point::new(1, 8)], cx)
        })
    });
    cx.simulate_keystrokes("ctrl-space");
    cx.run_until_parked();
    assert!(visible(&editor, cx));
    let first = labels(&editor, cx)[0].clone();
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(text(&editor, cx), format!("counter_total\n{first}\n"));
}

#[gpui::test]
fn auto_trigger_off_needs_the_shortcut(cx: &mut TestAppContext) {
    let mut config = config();
    config.editor.completion.auto_trigger = false;
    let (editor, cx) = open_at(cx, "manual_word\n", "test.rs", config);
    cx.simulate_input("man");
    assert!(!visible(&editor, cx));
    cx.simulate_keystrokes("ctrl-space");
    cx.run_until_parked();
    // The word, then the `main` snippet.
    assert_eq!(labels(&editor, cx), vec!["manual_word", "main"]);
}

#[gpui::test]
fn builtin_snippets_expand_with_tab_stops(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    cx.simulate_input("fn");
    let shown = labels(&editor, cx);
    assert!(shown.contains(&"fn".to_string()), "{shown:?}");
    let index = shown.iter().position(|l| l == "fn").unwrap();
    for _ in 0..index {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "fn name() {\n    \n}");
    // The first placeholder is selected: typing replaces it.
    cx.simulate_input("go");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("x: u8");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("body");
    assert_eq!(text(&editor, cx), "fn go(x: u8) {\n    body\n}");
}

#[gpui::test]
fn snippets_setting_hides_them(cx: &mut TestAppContext) {
    let mut config = config();
    config.editor.completion.snippets = false;
    let (editor, cx) = open_at(cx, "fnord\n", "test.rs", config);
    cx.simulate_input("fn");
    assert_eq!(labels(&editor, cx), vec!["fnord"]);
}

#[gpui::test]
fn user_snippet_choices_open_a_list(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    let toml = "[[snippet]]\nprefix = \"logl\"\nbody = \"log::${1|info,warn,error|}!(\\\"$2\\\");$0\"\ndescription = \"log line\"\nlanguages = [\"rust\"]\n";
    let loaded = cx.update(|_, cx| arcode_editor::completion::load_user_snippets(toml, cx));
    assert_eq!(loaded.unwrap(), 1);
    cx.simulate_input("logl");
    assert_eq!(labels(&editor, cx), vec!["logl"]);
    let docs = editor.read_with(cx, |e, _| {
        e.selected_completion().unwrap().candidate.detail.clone()
    });
    assert_eq!(docs.as_deref(), Some("log line"));
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "log::info!(\"\");");
    // The choice placeholder lists its options.
    assert_eq!(labels(&editor, cx), vec!["info", "warn", "error"]);
    cx.simulate_keystrokes("down enter");
    assert_eq!(text(&editor, cx), "log::warn!(\"\");");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("hi");
    assert_eq!(text(&editor, cx), "log::warn!(\"hi\");");
    // A broken file keeps the built-ins.
    let err = cx.update(|_, cx| {
        arcode_editor::completion::load_user_snippets("[[snippet]]\nprefix = 1", cx)
    });
    assert!(err.is_err());
    let error = cx.update(|_, cx| arcode_editor::completion::user_snippets_error(cx));
    assert!(error.is_some());
}

#[gpui::test]
fn paths_complete_inside_string_literals(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("readme.md"), "").unwrap();
    std::fs::write(dir.path().join("assets/logo.svg"), "").unwrap();
    let file = dir.path().join("main.rs");
    let (editor, cx) = open_at(cx, "let p = ", file.to_str().unwrap(), config());
    cx.simulate_input("\"./");
    assert_eq!(labels(&editor, cx), vec!["assets/", "readme.md"]);
    cx.simulate_input("as");
    assert_eq!(labels(&editor, cx), vec!["assets/"]);
    cx.simulate_keystrokes("enter");
    assert!(
        text(&editor, cx).ends_with("\"./assets/\""),
        "{}",
        text(&editor, cx)
    );
    // Accepting a folder lists its entries.
    assert_eq!(labels(&editor, cx), vec!["logo.svg"]);
    // `//` in a comment is not a path.
    cx.simulate_keystrokes("escape end enter");
    cx.simulate_input("//");
    assert!(!visible(&editor, cx));
}
