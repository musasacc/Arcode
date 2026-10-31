//! A renamed or moved file: the bridge follows the editor to the new path.

use std::path::PathBuf;

use arcode_buffer::Buffer;
use arcode_editor::Editor;
use arcode_lsp::LspEvent;
use arcode_lsp::lsp_types::{Diagnostic, Range};
use gpui::{AppContext, TestAppContext};

use super::LspBridge;

#[gpui::test]
fn a_renamed_file_is_reopened_under_its_new_path(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let bridge = cx.new(LspBridge::new);
    // No real servers: the document is tracked but not synced.
    bridge.update(cx, |b, _| b.set_enabled(false));
    let old = PathBuf::from("/proj/src/old.rs");
    let new = PathBuf::from("/proj/src/new.rs");
    let editor = cx.new(|cx| {
        let mut buffer = Buffer::new("fn main() {}\n");
        buffer.set_path(&old);
        Editor::new(buffer, cx)
    });
    bridge.update(cx, |b, cx| {
        b.register_editor(&editor, cx);
        b.handle_event(
            LspEvent::Diagnostics {
                server: "rust-analyzer".into(),
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
    // The file tree's rename only points the editor at the new path.
    editor.update(cx, |e, cx| e.set_path(&new, cx));
    cx.run_until_parked();
    bridge.read_with(cx, |b, _| {
        assert_eq!(b.doc_path(&editor), Some(new.as_path()));
        assert!(
            b.store().for_path(&old).is_empty(),
            "diagnostics of the closed document are dropped"
        );
    });
    // Reporting it again (as refactorings do) changes nothing.
    bridge.update(cx, |b, cx| b.editor_path_changed(&editor, cx));
    bridge.read_with(cx, |b, _| {
        assert_eq!(b.doc_path(&editor), Some(new.as_path()))
    });
}
