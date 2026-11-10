use std::path::Path;
use std::sync::Arc;

use arcode_config::{Config, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, FileIcons, ThemeRegistry};
use arcode_ui_kit::{FileGlyph, FileLogo, UiTheme, logo_rasterizations};
use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px};

use super::*;

fn themes() -> Vec<UiTheme> {
    let registry = ThemeRegistry::builtin();
    [DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME]
        .into_iter()
        .map(|name| UiTheme::new(Arc::new(registry.get(name).unwrap().clone())))
        .collect()
}

fn plain_config() -> Config {
    Config::from_toml_str("[ui]\nfile_icons = \"plain\"\n").0
}

#[test]
fn kinds_of_common_files() {
    assert_eq!(file_kind("main.rs"), FileKind::Source);
    assert_eq!(file_kind("App.TSX"), FileKind::Source);
    assert_eq!(file_kind("Cargo.toml"), FileKind::Manifest);
    assert_eq!(file_kind("Cargo.lock"), FileKind::Manifest);
    assert_eq!(file_kind("README.md"), FileKind::Doc);
    assert_eq!(file_kind("notes.txt"), FileKind::Doc);
    assert_eq!(file_kind("rustfmt.toml"), FileKind::Other);
    assert_eq!(file_kind(".gitignore"), FileKind::Other);
    assert_eq!(file_kind("Makefile"), FileKind::Other);
}

/// `ui.file_icons = "logos"`: language files show their logo (tinted logos
/// in `text`), the rest keep the plain icon and its tint by kind.
#[test]
fn logos_for_language_files_plain_icons_for_the_rest() {
    let config = Config::default();
    for theme in themes() {
        let c = &theme.colors;
        let parts = |name: &str| file_icon_parts(Path::new(name), &config, c);
        assert_eq!(parts("main.rs"), (FileGlyph::Logo(FileLogo::Rust), c.text));
        assert_eq!(parts("app.py"), (FileGlyph::Logo(FileLogo::Python), c.text));
        assert_eq!(
            parts("/p/Cargo.toml"),
            (FileGlyph::Logo(FileLogo::Rust), c.text)
        );
        assert_eq!(
            parts("notes.txt"),
            (FileGlyph::Icon(IconName::FileText), c.file_doc)
        );
        assert_eq!(
            parts("rustfmt.cfg"),
            (FileGlyph::Icon(IconName::Settings), c.text_muted)
        );
        assert_eq!(
            parts("logo.png"),
            (FileGlyph::Icon(IconName::File), c.text_muted)
        );
    }
}

/// `ui.file_icons = "plain"`: the plain icons everywhere, as before logos.
#[test]
fn the_plain_setting_turns_logos_off() {
    let config = plain_config();
    assert_eq!(config.ui.file_icons, FileIcons::Plain);
    for theme in themes() {
        let c = &theme.colors;
        let parts = |name: &str| file_icon_parts(Path::new(name), &config, c);
        assert_eq!(
            parts("main.rs"),
            (FileGlyph::Icon(IconName::FileText), c.file_source)
        );
        assert_eq!(
            parts("Cargo.toml"),
            (FileGlyph::Icon(IconName::Package), c.file_manifest)
        );
        assert_eq!(
            parts(".gitignore"),
            (FileGlyph::Icon(IconName::GitBranch), c.text_muted)
        );
        assert_eq!(
            parts("README.md"),
            (FileGlyph::Icon(IconName::FileText), c.file_doc)
        );
    }
}

/// Section 1: ember marks where you are; a file icon is never ember, and
/// never a status colour (2.4 rule 7), with logos or without.
#[test]
fn file_icons_never_use_ember_or_status_colours() {
    for config in [Config::default(), plain_config()] {
        for theme in themes() {
            let c = &theme.colors;
            for name in [
                "main.py", "lib.c", "main.go", "run.sh", "app.ts", "index.js", "x.bin",
            ] {
                let (_, color) = file_icon_parts(Path::new(name), &config, c);
                for status in [c.accent, c.warning, c.error, c.success] {
                    assert_ne!(color, status, "{}: {name}", theme.name);
                }
            }
        }
    }
}

/// A view drawing `rows` file rows, cycling through `names`.
struct ManyFiles {
    names: Vec<&'static str>,
    rows: usize,
}

impl Render for ManyFiles {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().children((0..self.rows).map(|ix| {
            let name = self.names[ix % self.names.len()];
            div()
                .flex()
                .h(px(24.))
                .child(file_icon(name, cx).size(px(15.)))
                .child(name)
        }))
    }
}

/// Thousands of file rows rasterise each colour logo once per size; later
/// frames only look the images up.
#[gpui::test]
fn logos_are_rasterised_once_not_per_frame(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let names = vec![
        "main.rs",
        "app.py",
        "index.js",
        "App.tsx",
        "main.go",
        "lib.c",
        "Main.java",
        "README.md",
        "notes.txt",
        "Dockerfile",
    ];
    let colour = names
        .iter()
        .filter_map(|n| logo_for(Path::new(n), &Config::default()))
        .filter(|l| l.style() == arcode_ui_kit::LogoStyle::Colour)
        .count();
    let (_view, cx) = cx.add_window_view(|_, _| ManyFiles { names, rows: 3000 });
    cx.update(|window, cx| window.draw(cx).clear());
    let first = cx.update(|_, cx| logo_rasterizations(cx));
    assert_eq!(first, colour, "one image per colour logo at this size");
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear();
        });
    }
    assert_eq!(cx.update(|_, cx| logo_rasterizations(cx)), first);
}
