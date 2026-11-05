//! Readable rows in popovers (docs/DESIGN.md 3, 6.12): the `…` list of
//! open tabs and any list of rows drawn in a `deferred` popover keep
//! their text at 4.5:1 or more against what is behind it, in every
//! built-in theme. GPUI 0.2.2 paints deferred elements without the text
//! style of their ancestors, which once drew the unselected rows of the
//! tab list in black on Ember.

use std::fs;
use std::sync::Arc;

use arcode_config::theme::BUILTIN_THEME_NAMES;
use arcode_config::{Color, ThemeRegistry};
use arcode_ui_kit::paint_log::{self, PaintedLabel};
use arcode_ui_kit::{list_row, popover, set_theme, truncated};
use gpui::{
    Bounds, Context, Hsla, IntoElement, Pixels, Render, Rgba, TestAppContext, Window, anchored,
    deferred, div, prelude::*, px,
};

use super::tests::{open, project_dir};

/// WCAG AA for body text.
const MIN_CONTRAST: f32 = 4.5;

fn color(h: Hsla) -> Color {
    let c = Rgba::from(h);
    let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::rgba(byte(c.r), byte(c.g), byte(c.b), byte(c.a))
}

/// The labels painted inside `area`.
fn painted_in(area: Bounds<Pixels>) -> Vec<PaintedLabel> {
    paint_log::take()
        .into_iter()
        .filter(|l| area.contains(&l.bounds.center()))
        .collect()
}

fn assert_readable(theme: &str, label: &PaintedLabel, background: Color, behind: &str) {
    let ratio = color(label.color).contrast_ratio(background);
    assert!(
        ratio >= MIN_CONTRAST,
        "{theme}: `{}` is drawn at {ratio:.2}:1 on {behind} (text {:?})",
        label.text,
        color(label.color),
    );
}

#[gpui::test]
fn tab_list_rows_are_readable_in_every_theme(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
        ws.open_file(&root.join("src/lib.rs"), window, cx);
    });
    cx.run_until_parked();
    let registry = ThemeRegistry::builtin();
    for name in BUILTIN_THEME_NAMES {
        let theme = registry.get(name).expect("built-in theme").clone();
        let ui = theme.ui;
        cx.update(|_, cx| set_theme(Arc::new(theme), cx));
        paint_log::start();
        ws.update(cx, |ws, cx| {
            ws.panes()[0].update(cx, |pane, cx| pane.open_tab_menu(cx))
        });
        cx.run_until_parked();
        let menu = cx
            .debug_bounds("tab-menu")
            .unwrap_or_else(|| panic!("{name}: the tab list was not drawn"));
        let rows = painted_in(menu);
        let row = |text: &str| {
            rows.iter()
                .find(|l| l.text == text)
                .unwrap_or_else(|| panic!("{name}: no `{text}` row in the tab list: {rows:?}"))
        };
        // lib.rs is the active tab: its row sits on the pill.
        assert_readable(name, row("main.rs"), ui.surface_raised, "the popover");
        assert_readable(name, row("lib.rs"), ui.pill, "the selected row");
    }
}

/// A popover of plain and selected rows, drawn deferred as menus are,
/// under a root that sets no text style of its own.
struct RowsPopover;

impl Render for RowsPopover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = popover(cx)
            .debug_selector(|| "rows".into())
            .w(px(240.))
            .child(
                list_row("plain", false, cx)
                    .mx(px(0.))
                    .child(truncated("plain row")),
            )
            .child(
                list_row("selected", true, cx)
                    .mx(px(0.))
                    .child(truncated("selected row")),
            );
        div()
            .size_full()
            .child(deferred(anchored().child(list)).with_priority(1))
    }
}

#[gpui::test]
fn list_rows_in_a_deferred_popover_are_readable_in_every_theme(cx: &mut TestAppContext) {
    let (_view, cx) = open_rows(cx);
    let registry = ThemeRegistry::builtin();
    for name in BUILTIN_THEME_NAMES {
        let theme = registry.get(name).expect("built-in theme").clone();
        let ui = theme.ui;
        cx.update(|_, cx| set_theme(Arc::new(theme), cx));
        paint_log::start();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let area = cx.debug_bounds("rows").expect("the popover was drawn");
        let rows = painted_in(area);
        let row = |text: &str| {
            rows.iter()
                .find(|l| l.text == text)
                .unwrap_or_else(|| panic!("{name}: `{text}` was not painted: {rows:?}"))
        };
        assert_readable(name, row("plain row"), ui.surface_raised, "the popover");
        assert_readable(name, row("selected row"), ui.pill, "the selected row");
    }
}

/// A label in a deferred element made of plain `div`s, which set no text
/// style of their own (the editor's popovers, the menu bar backdrop): only
/// the ancestors of the deferred element give it a colour.
struct PlainDeferredLabel(Hsla);

impl Render for PlainDeferredLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let inner = div()
            .debug_selector(|| "plain".into())
            .w(px(200.))
            .child(div().child(truncated("plain label")));
        div()
            .size_full()
            .text_color(self.0)
            .text_size(px(13.))
            .child(deferred(anchored().child(inner)).with_priority(1))
    }
}

/// `truncated` reads the text style while laying out, where GPUI still
/// has the ancestors' style; reading it while painting a deferred element
/// got GPUI's default black.
#[gpui::test]
fn a_deferred_label_keeps_the_colour_of_its_ancestors(cx: &mut TestAppContext) {
    super::tests::init(cx);
    let registry = ThemeRegistry::builtin();
    for name in BUILTIN_THEME_NAMES {
        let ui = registry.get(name).expect("built-in theme").clone().ui;
        let text = arcode_ui_kit::hsla(ui.text);
        let (_view, cx) = cx.add_window_view(|_, _| PlainDeferredLabel(text));
        paint_log::start();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let area = cx.debug_bounds("plain").expect("the label was drawn");
        let label = painted_in(area)
            .into_iter()
            .find(|l| l.text == "plain label")
            .unwrap_or_else(|| panic!("{name}: the label was not painted"));
        assert_eq!(color(label.color), color(text), "{name}");
        assert_readable(name, &label, ui.surface_raised, "a popover");
    }
}

fn open_rows(cx: &mut TestAppContext) -> (gpui::Entity<RowsPopover>, &mut gpui::VisualTestContext) {
    super::tests::init(cx);
    cx.add_window_view(|_, _| RowsPopover)
}
