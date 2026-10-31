//! Review tests for `LspLanguageServices`. Each reproduces a bug found in
//! review and fails until it is fixed.

use arcode_buffer::{Buffer, Point};
use arcode_editor::Editor;
use arcode_lsp::PositionEncoding;
use gpui::{AppContext, TestAppContext};

use super::LspLanguageServices;

/// `inlay_hints` asks for `Point::new(lines.start, 0)..Point::new(lines.end,
/// 0)`. When the wanted lines reach the end of the buffer (`lines.end ==
/// line_count`, which `wanted_lines` produces for every short file and at
/// the bottom of every file), `Point::new(line_count, 0)` is clamped by
/// `point_to_offset` to the *start* of the last line, so the last line is
/// never asked for. A file without a trailing newline gets no hints on its
/// last line, and a one-line file gets an empty range (no hints at all).
#[gpui::test]
fn inlay_range_covers_the_last_line(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let text = "fn main() { let x = vec![1]; }";
    let editor = cx.new(|cx| Editor::new(Buffer::new(text), cx));
    editor.read_with(cx, |editor, _| {
        // What `inlay_hints` builds for `lines = 0..line_count`.
        let lines = 0..editor.buffer().line_count();
        let range = Point::new(lines.start, 0)..Point::new(lines.end, 0);
        let lsp = LspLanguageServices::lsp_range(editor, &range, PositionEncoding::Utf16);
        assert!(
            (lsp.end.line, lsp.end.character) >= (0, text.len() as u32),
            "the inlay hint request must cover the whole last line, got {lsp:?}"
        );
    });
}
