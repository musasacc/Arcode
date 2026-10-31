//! Review tests for the pill tabs of docs/DESIGN.md 6.6: the file icon a
//! tab (and the `…` tab list) shows comes from `file_icon`.

use std::sync::Arc;

use arcode_config::{DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, ThemeRegistry};
use arcode_ui_kit::UiTheme;

use crate::file_icons::{file_icon, file_icon_tinted};

fn themes() -> Vec<UiTheme> {
    let registry = ThemeRegistry::builtin();
    [DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME]
        .into_iter()
        .map(|name| UiTheme::new(Arc::new(registry.get(name).unwrap().clone())))
        .collect()
}

/// Section 1: ember marks where you are; a tab icon is never ember, and
/// never a status colour (2.4 rule 7).
#[test]
fn review_tab_icons_never_use_ember_or_status_colours() {
    for theme in themes() {
        let c = &theme.colors;
        for name in [
            "main.py", "lib.c", "main.go", "run.sh", "app.ts", "index.js",
        ] {
            let (_, color) = file_icon(name, c);
            assert_ne!(color, c.accent, "{}: {name} tab icon is ember", theme.name);
            assert_ne!(
                color, c.warning,
                "{}: {name} tab icon is the warning colour",
                theme.name
            );
        }
    }
}

/// dark_main.png / light_main.png: the `main.rs` tab icon has the same
/// tint as `main.rs` in the file tree (6.5: source files `syntax.type`).
#[test]
fn review_tab_icon_matches_the_file_tree_tint() {
    for theme in themes() {
        for name in ["main.rs", "Cargo.toml", "README.md"] {
            assert_eq!(
                file_icon(name, &theme.colors).1,
                file_icon_tinted(name, &theme).1,
                "{}: {name} tab icon differs from the tree",
                theme.name
            );
        }
    }
}
