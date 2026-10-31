//! Review tests for the rename/move follow-up in `paths.rs`. Each
//! reproduces a bug found in review and fails until it is fixed.

use arcode_buffer::Buffer;
use arcode_config::Config;
use arcode_editor::Editor;
use gpui::{AppContext, TestAppContext};

use super::LspBridge;

fn set_config(cx: &mut TestAppContext, toml: &str) {
    cx.update(|cx| {
        let (config, warnings) = Config::from_toml_str(toml);
        assert!(warnings.is_empty(), "{warnings:?}");
        arcode_ui_kit::set_settings(config, cx);
    });
}

/// `settings_changed` (servers.rs) re-registers editors whose language
/// gained or lost all servers by calling `editor_path_changed`. That now
/// returns early when the path did not change, so an open file keeps its
/// `didOpen` state (and completion/hover providers) after its language's
/// servers are removed from the settings, and a file whose language gains
/// a server is never opened on it.
#[gpui::test]
fn removing_a_languages_servers_closes_its_open_documents(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    set_config(
        cx,
        "[language.rust]\nservers = [\"nope\"]\n\
         [lsp.nope]\ncommand = \"arcode-review-no-such-binary\"\n",
    );
    let bridge = cx.new(LspBridge::new);
    let editor = cx.new(|cx| {
        let mut buffer = Buffer::new("fn main() {}\n");
        buffer.set_path("/proj/src/main.rs");
        Editor::new(buffer, cx)
    });
    bridge.update(cx, |b, cx| b.register_editor(&editor, cx));
    let synced = |bridge: &gpui::Entity<LspBridge>, cx: &mut TestAppContext| {
        bridge.read_with(cx, |b, _| b.docs.get(&editor.entity_id()).map(|d| d.synced))
    };
    assert_eq!(synced(&bridge, cx), Some(true), "rust has a server");
    // The user removes rust's servers in the settings.
    set_config(cx, "[language.rust]\nservers = []\n");
    cx.run_until_parked();
    assert_eq!(
        synced(&bridge, cx),
        Some(false),
        "the document must be closed on the removed server"
    );
}

/// `Workspace::paths_renamed` points the editor at the new path and then
/// calls `reapply_association`; when the association (or a language picked
/// by hand) differs from the language `Editor::set_path` derived from the
/// new extension, `set_editor_language` calls `editor_language_changed`
/// synchronously, before the `PathChanged` event is delivered. That
/// re-registers the editor under the new path but clears diagnostics only
/// for the *new* path; `editor_path_changed` then sees the new path
/// already tracked and returns early, so the old path's diagnostics stay
/// in the store (Problems panel, status bar count) forever.
#[gpui::test]
fn rename_with_language_change_drops_old_path_diagnostics(cx: &mut TestAppContext) {
    use arcode_lsp::LspEvent;
    use arcode_lsp::lsp_types::{Diagnostic, Range};
    use std::path::PathBuf;

    crate::test_support::init(cx);
    let bridge = cx.new(LspBridge::new);
    bridge.update(cx, |b, _| b.set_enabled(false));
    let old = PathBuf::from("/proj/src/a.php");
    let new = PathBuf::from("/proj/src/a.inc");
    let editor = cx.new(|cx| {
        let mut buffer = Buffer::new("<?php\n");
        buffer.set_path(&old);
        Editor::new(buffer, cx)
    });
    bridge.update(cx, |b, cx| {
        b.register_editor(&editor, cx);
        b.handle_event(
            LspEvent::Diagnostics {
                server: "intelephense".into(),
                path: old.clone(),
                version: None,
                diagnostics: vec![Diagnostic {
                    range: Range::default(),
                    message: "stale".into(),
                    ..Default::default()
                }],
            },
            cx,
        );
    });
    // What `paths_renamed` + `reapply_association` do in one update.
    cx.update(|cx| {
        editor.update(cx, |e, cx| e.set_path(&new, cx));
        bridge.update(cx, |b, cx| b.editor_language_changed(&editor, cx));
    });
    cx.run_until_parked();
    bridge.read_with(cx, |b, _| {
        assert_eq!(
            b.docs.get(&editor.entity_id()).map(|d| d.path.as_path()),
            Some(new.as_path())
        );
        assert!(
            b.store().for_path(&old).is_empty(),
            "diagnostics of the renamed file's old path must be dropped"
        );
    });
}
