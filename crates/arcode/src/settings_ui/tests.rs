//! The Settings page draws every section, and its rows write the file.

use gpui::{TestAppContext, VisualTestContext};

use super::*;
use crate::prefs::store::ConfigPathOverride;

#[gpui::test]
fn every_section_draws(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
    let (page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    for section in Section::ALL {
        page.update(cx, |page, cx| page.set_section(section, cx));
        cx.run_until_parked();
        let _: &mut VisualTestContext = cx;
        cx.update(|window, cx| window.draw(cx).clear());
        assert_eq!(page.read_with(cx, |p, _| p.section()), section);
    }
}

#[gpui::test]
fn low_end_switch_writes_the_preset(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
    let (page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    page.update(cx, |page, cx| {
        page.writer().write(arcode_config::low_end_edits(true), cx)
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let config = arcode_ui_kit::settings(cx);
        assert!(config.ui.low_end_mode);
        assert!(!config.editor.inlay_hints && !config.git.inline_blame);
        assert!(arcode_ui_kit::low_end(cx));
        assert_eq!(
            arcode_ui_kit::debounce(250, cx),
            std::time::Duration::from_millis(750)
        );
    });
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(
        text.starts_with("# Arcode configuration"),
        "a missing file starts from the template"
    );
}

/// Theme names longer than a card (`Arcode Midnight`) stay inside it: the
/// label shrinks to the card and ends in `…` (see `truncated`), with the
/// full name as the card's tooltip.
#[gpui::test]
fn theme_card_labels_fit_their_cards(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    cx.update(|cx| cx.set_global(ConfigPathOverride(dir.path().join("config.toml"))));
    let (_page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let label = cx
        .debug_bounds("theme-card-label")
        .expect("a theme card label is drawn");
    // The label shares the card's width with the ember diamond (7 px and
    // a 6 px gap) on the selected card.
    assert!(label.size.width > px(0.));
    assert!(
        label.size.width <= px(super::theme_card::CARD_W + 3.),
        "{label:?}"
    );
}

/// The page filter shows the matching rows of every section and hides
/// the rest; picking a section in the nav ends it.
#[gpui::test]
fn filter_searches_every_section(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    cx.update(|cx| cx.set_global(ConfigPathOverride(dir.path().join("config.toml"))));
    let (page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    page.update(cx, |page, cx| {
        page.filter.update(cx, |f, cx| f.set_text("low-end", cx))
    });
    cx.run_until_parked();
    assert!(page.read_with(cx, |p, cx| p.filtering(cx)));
    cx.update(|window, cx| window.draw(cx).clear());
    // Theme cards are not drawn while filtering.
    assert!(cx.debug_bounds("theme-card-label").is_none());
    // Nothing of the filter outlives the draw.
    assert!(!super::filter::active());
    page.update_in(cx, |page, window, cx| {
        page.clear_filter(window, cx);
        page.set_section(Section::Editor, cx)
    });
    cx.run_until_parked();
    assert!(!page.read_with(cx, |p, cx| p.filtering(cx)));
    cx.update(|window, cx| window.draw(cx).clear());
}
