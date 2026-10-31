//! Standalone editor window.
//!
//! ```text
//! cargo run -p arcode-editor --example editor -- [FILE] [--lines N] [--fake-diagnostics] [--blink]
//! ```
//!
//! Without FILE a Rust snippet is generated (`--lines N` makes it N lines
//! long). `--fake-diagnostics` (or `ARCODE_FAKE_DIAGNOSTICS=1`) injects a few
//! diagnostics so underlines and the hover popover can be inspected.
//! `RUST_LOG=arcode_editor=trace` logs per-frame layout times.

use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{DiagnosticSeverity, Editor, EditorDiagnostic, EditorEvent};
use gpui::{
    App, Application, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
    px, size,
};

fn generated(lines: usize) -> String {
    let mut text = String::from(
        "//! Generated sample.\nuse std::collections::HashMap;\n\nfn main() {\n    let mut map: HashMap<String, u32> = HashMap::new();\n    map.insert(\"answer\".to_string(), 42);\n\tprintln!(\"{:?}\", map);\n    let unused = undefined_value + 1;\n}\n",
    );
    let mut count = text.lines().count();
    let mut i = 0;
    while count < lines {
        text.push_str(&format!(
            "\n/// Function number {i}.\npub fn function_{i}(x: u64) -> u64 {{\n    let value = x.wrapping_mul({i}) ^ 0x9e37_79b9; // àé 漢字\n    if value % 2 == 0 {{ return value + {i}; }}\n    value\n}}\n"
        ));
        count += 7;
        i += 1;
    }
    text
}

fn fake_diagnostics() -> Vec<EditorDiagnostic> {
    let d = |a: (usize, usize), b: (usize, usize), severity, message: &str| EditorDiagnostic {
        range: Point::new(a.0, a.1)..Point::new(b.0, b.1),
        severity,
        message: message.to_string(),
        source: "rust-analyzer".to_string(),
        code: None,
    };
    vec![
        d(
            (7, 17),
            (7, 32),
            DiagnosticSeverity::Error,
            "cannot find value `undefined_value` in this scope",
        ),
        d(
            (7, 8),
            (7, 14),
            DiagnosticSeverity::Warning,
            "unused variable: `unused`",
        ),
        d(
            (4, 8),
            (5, 7),
            DiagnosticSeverity::Info,
            "variable does not need to be mutable (multi-line demo)",
        ),
        d(
            (3, 11),
            (3, 11),
            DiagnosticSeverity::Hint,
            "zero-width hint at end of line",
        ),
    ]
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take_flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .map(|i| args.remove(i))
            .is_some()
    };
    let fake =
        take_flag("--fake-diagnostics") || std::env::var_os("ARCODE_FAKE_DIAGNOSTICS").is_some();
    let blink = take_flag("--blink");
    let lines = args
        .iter()
        .position(|a| a == "--lines")
        .and_then(|i| {
            let n = args.get(i + 1)?.parse().ok();
            args.drain(i..=i + 1);
            n
        })
        .unwrap_or(60);
    let buffer = match args.first() {
        Some(path) => Buffer::from_file(path).unwrap_or_else(|err| {
            eprintln!("failed to open {path}: {err}");
            std::process::exit(1);
        }),
        None => {
            let mut b = Buffer::new(&generated(lines));
            b.set_path(std::env::temp_dir().join("arcode-generated.rs"));
            b
        }
    };

    Application::new()
        .with_assets(arcode_ui_kit::Assets)
        .run(move |cx: &mut App| {
            arcode_ui_kit::load_fonts(cx);
            let mut config = Config::default();
            config.editor.cursor_blink = blink;
            arcode_ui_kit::set_settings(config, cx);
            let registry = ThemeRegistry::builtin();
            let theme = registry
                .get("Arcode Dark")
                .cloned()
                .unwrap_or_else(|| registry.fallback(arcode_config::Appearance::Dark).clone());
            arcode_ui_kit::set_theme(Arc::new(theme), cx);
            arcode_editor::init(cx);

            let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Arcode editor example".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let opened = cx.open_window(options, |window, cx| {
                let editor = cx.new(|cx| {
                    let mut editor = Editor::new(buffer, cx);
                    if fake {
                        editor.set_diagnostics(fake_diagnostics(), cx);
                    }
                    editor
                });
                cx.subscribe(&editor, |_, event: &EditorEvent, _| {
                    if let EditorEvent::Edited { edits, version } = event {
                        log::info!("edited: {} edit(s), version {version}", edits.len());
                    }
                })
                .detach();
                window.focus(&editor.focus_handle(cx));
                editor
            });
            if let Err(err) = opened {
                eprintln!("failed to open window: {err:#}");
                cx.quit();
            }
            cx.activate(true);
        });
}
