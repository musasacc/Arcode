//! File icons in the workspace (docs/DESIGN.md 6.5, 6.6): tabs, the `…`
//! tab list and the file tree show the language logo of each file, and
//! `ui.file_icons = "plain"` brings the plain icons back.

use std::fs;

use arcode_config::{Config, FileIcons};
use arcode_ui_kit::{FileGlyph, FileLogo, IconName, logo_rasterizations, set_settings};
use gpui::TestAppContext;

use super::tests::{open, project_dir};

#[gpui::test]
fn tabs_show_the_logo_of_their_file(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("app.py"), "print(1)\n").expect("write");
    fs::write(root.join("notes.txt"), "hi\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        for name in ["src/main.rs", "app.py", "notes.txt"] {
            ws.open_file(&root.join(name), window, cx);
        }
    });
    cx.run_until_parked();
    let glyphs = |cx: &mut gpui::VisualTestContext| {
        ws.update(cx, |ws, cx| {
            let pane = ws.panes()[0].clone();
            let pane = pane.read(cx);
            pane.items()
                .iter()
                .map(|editor| {
                    let title = editor.read(cx).title();
                    pane.tab_glyph(editor, &title, cx).0
                })
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        glyphs(cx),
        [
            FileGlyph::Logo(FileLogo::Rust),
            FileGlyph::Logo(FileLogo::Python),
            FileGlyph::Icon(IconName::FileText),
        ]
    );
    // The window drew the Python logo (tabs and tree) as an image, once.
    cx.update(|window, cx| window.draw(cx).clear());
    let drawn = cx.update(|_, cx| logo_rasterizations(cx));
    assert!(drawn >= 1, "no colour logo was drawn");
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
    assert_eq!(cx.update(|_, cx| logo_rasterizations(cx)), drawn);

    cx.update(|_, cx| {
        let mut config = Config::default();
        config.ui.file_icons = FileIcons::Plain;
        set_settings(config, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        glyphs(cx),
        [
            FileGlyph::Icon(IconName::FileText),
            FileGlyph::Icon(IconName::FileText),
            FileGlyph::Icon(IconName::FileText),
        ]
    );
}
