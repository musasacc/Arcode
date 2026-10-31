//! Review checks for the Sidebars lane of the redesign (docs/DESIGN.md
//! 2.4, 4 and 6.5). They read the sources, because the offending styles
//! (a Tailwind text size, an alpha fill, GPUI's `truncate()`) leave no trace
//! a unit test can query once an element is built.

const TREE_RENDER: &str = include_str!("../src/sidebar/file_tree/render.rs");
const TODO_VIEW: &str = include_str!("../src/todo/view.rs");
const PACKAGE_ROWS: &str = include_str!("../src/packages/view_rows.rs");
const PLUGIN_CARD: &str = include_str!("../src/plugins/page/card.rs");
const PLUGIN_LAYOUT: &str = include_str!("../src/plugins/panel/layout.rs");
const PLUGIN_PANEL: &str = include_str!("../src/plugins/panel/mod.rs");
const PLUGIN_TABLE: &str = include_str!("../src/plugins/panel/table.rs");
const PLUGIN_TABLE_SCROLL: &str = include_str!("../src/plugins/panel/table_scroll.rs");

fn lines_with<'a>(source: &'a str, needle: &str) -> Vec<(usize, &'a str)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle) && !l.trim_start().starts_with("//"))
        .map(|(i, l)| (i + 1, l.trim()))
        .collect()
}

/// Section 4: tree rows are "body" (font_size = 13, from `type_scale`).
/// `text_sm()` is 0.875 rem = 14 px at GPUI's default 16 px rem, and does
/// not follow `ui.font_size`.
#[test]
fn tree_rows_use_the_body_size_not_text_sm() {
    let hits = lines_with(TREE_RENDER, ".text_sm()");
    assert!(
        hits.is_empty(),
        "file_tree/render.rs uses text_sm(): {hits:?}"
    );
}

/// Section 2 / 2.4 rule 6: nothing in the chrome uses alpha.
#[test]
fn plugin_views_have_no_translucent_fills() {
    let mut hits = Vec::new();
    for (file, src) in [
        ("plugins/panel/layout.rs", PLUGIN_LAYOUT),
        ("plugins/panel/mod.rs", PLUGIN_PANEL),
        ("plugins/panel/table.rs", PLUGIN_TABLE),
        ("plugins/panel/table_scroll.rs", PLUGIN_TABLE_SCROLL),
    ] {
        for (line, text) in lines_with(src, ".opacity(") {
            hits.push(format!("{file}:{line}: {text}"));
        }
    }
    assert!(hits.is_empty(), "alpha colours in plugin views: {hits:#?}");
}

/// Lane check "long names truncate with ellipsis": GPUI 0.2.2's
/// `truncate()` cuts these names with no "…" inside flex rows (see the
/// note in plugins/panel/layout.rs); the kit's `truncated()` does it right.
#[test]
fn long_names_use_the_kit_truncation() {
    let mut hits = Vec::new();
    for (file, src) in [
        ("todo/view.rs", TODO_VIEW),
        ("packages/view_rows.rs", PACKAGE_ROWS),
        ("plugins/page/card.rs", PLUGIN_CARD),
    ] {
        for (line, text) in lines_with(src, ".truncate()") {
            hits.push(format!("{file}:{line}: {text}"));
        }
    }
    assert!(hits.is_empty(), "names cut without an ellipsis: {hits:#?}");
}
