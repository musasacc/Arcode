//! Unused and deprecated code in a drawn editor: the shaped lines carry
//! faded and struck runs over the tagged ranges, in every built-in theme,
//! and `editor.fade_unused = false` turns the fading off.

use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, InlineDiagnostics, ThemeRegistry};
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

use crate::diagnostics::{lens_diagnostics, line_severities};
use crate::{DiagnosticSeverity, DiagnosticTags, Editor, EditorDiagnostic, UNNECESSARY_OPACITY};

const TEXT: &str = "use std::fmt;\nfn main() {\n    let x = old();\n}\n";

fn diag(
    line: usize,
    cols: (usize, usize),
    severity: DiagnosticSeverity,
    unnecessary: bool,
    deprecated: bool,
) -> EditorDiagnostic {
    EditorDiagnostic {
        range: Point::new(line, cols.0)..Point::new(line, cols.1),
        severity,
        message: "tagged".into(),
        source: "test".into(),
        code: None,
        tags: DiagnosticTags {
            unnecessary,
            deprecated,
        },
    }
}

/// Unused import (a hint), unused variable (a warning), deprecated call.
fn tagged() -> Vec<EditorDiagnostic> {
    vec![
        diag(0, (0, 13), DiagnosticSeverity::Hint, true, false),
        diag(2, (8, 9), DiagnosticSeverity::Warning, true, false),
        diag(2, (12, 15), DiagnosticSeverity::Warning, false, true),
    ]
}

fn open(cx: &mut TestAppContext, config: Config) -> (Entity<Editor>, &mut VisualTestContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(config, cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        let mut buffer = Buffer::new(TEXT);
        buffer.set_path("main.rs");
        Editor::new(buffer, cx)
    });
    editor.update(cx, |e, cx| e.set_diagnostics(tagged(), cx));
    // Highlights arrive from a background job after the first frames.
    for _ in 0..4 {
        draw(cx);
    }
    (editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

/// What each char column of `line` looks like: (colour, struck).
fn styled_cols(editor: &Entity<Editor>, line: usize, cx: &mut VisualTestContext) -> Vec<Look> {
    editor.read_with(cx, |e, _| {
        let layout = e.layout.as_ref().expect("drawn");
        let v = layout.visible(line).expect("line in view");
        let len = e.buffer.line_len(line);
        (0..len)
            .map(|col| {
                let byte = v.display.char_start(col);
                let mut at = 0;
                let run = v
                    .display
                    .runs
                    .iter()
                    .find(|r| {
                        at += r.len;
                        byte < at
                    })
                    .expect("run");
                Look {
                    color: run.color,
                    struck: run.strikethrough.and_then(|s| s.color),
                }
            })
            .collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    color: Hsla,
    struck: Option<Hsla>,
}

fn faded(look: &Look) -> bool {
    (look.color.a - UNNECESSARY_OPACITY).abs() < 1e-4
}

#[gpui::test]
fn tagged_ranges_are_faded_and_struck_in_every_theme(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, Config::default());
    let registry = ThemeRegistry::builtin();
    let names = registry.names();
    assert!(names.contains(&"Arcode Ember") && names.contains(&"Arcode Chalk"));
    for name in names {
        let theme = registry.get(name).expect(name).clone();
        cx.update(|_, cx| arcode_ui_kit::set_theme(Arc::new(theme), cx));
        editor.update(cx, |_, cx| cx.notify());
        draw(cx);
        let text = cx.update(|_, cx| arcode_ui_kit::theme(cx).colors.text);
        // The whole unused import is faded, in its syntax colours.
        let import = styled_cols(&editor, 0, cx);
        assert!(import.iter().all(faded), "{name}: {import:?}");
        assert!(
            import.iter().any(|l| l.color.opacity(1.) != text),
            "{name}: keeps the syntax colours"
        );
        // `x` is faded, `old` struck, the rest untouched.
        let body = styled_cols(&editor, 2, cx);
        for (col, look) in body.iter().enumerate() {
            assert_eq!(faded(look), col == 8, "{name}: col {col} {look:?}");
            let struck = (12..15).contains(&col);
            assert_eq!(look.struck, struck.then_some(text), "{name}: col {col}");
        }
        let main = styled_cols(&editor, 1, cx);
        assert!(main.iter().all(|l| !faded(l) && l.struck.is_none()));
    }
}

#[gpui::test]
fn fade_setting_off_keeps_only_the_strikethrough(cx: &mut TestAppContext) {
    let mut config = Config::default();
    config.editor.fade_unused = false;
    let (editor, cx) = open(cx, config);
    let import = styled_cols(&editor, 0, cx);
    assert!(import.iter().all(|l| !faded(l)), "{import:?}");
    let body = styled_cols(&editor, 2, cx);
    assert!(body.iter().all(|l| !faded(l)), "{body:?}");
    assert!(body[12].struck.is_some() && body[11].struck.is_none());
    // Turning it on again takes effect on the next frame.
    cx.update(|_, cx| arcode_ui_kit::set_settings(Config::default(), cx));
    editor.update(cx, |_, cx| cx.notify());
    draw(cx);
    assert!(styled_cols(&editor, 0, cx).iter().all(faded));
}

#[test]
fn hints_that_only_fade_get_no_gutter_dot_or_lens() {
    let ds = tagged();
    // Line 0 holds only the fade-only hint; line 2 has warnings.
    let dots = line_severities(&ds, 0..3, true);
    assert_eq!(dots, vec![None, None, Some(DiagnosticSeverity::Warning)]);
    let dots = line_severities(&ds, 0..3, false);
    assert_eq!(
        dots[0],
        Some(DiagnosticSeverity::Hint),
        "fading off: a plain hint"
    );
    let lens = lens_diagnostics(&ds, 0..3, InlineDiagnostics::All, true);
    assert!(lens[0].is_none() && lens[2].is_some());
    let lens = lens_diagnostics(&ds, 0..3, InlineDiagnostics::All, false);
    assert!(lens[0].is_some());
}
