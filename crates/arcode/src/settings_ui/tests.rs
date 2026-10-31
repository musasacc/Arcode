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
/// full name as the card's tooltip. On the selected card the label also
/// leaves room for the ember diamond (7 px and a 6 px gap) before it.
#[gpui::test]
fn theme_card_labels_fit_their_cards(cx: &mut TestAppContext) {
    use super::theme_card::{card_selector, label_selector};
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    cx.update(|cx| cx.set_global(ConfigPathOverride(dir.path().join("config.toml"))));
    let (_page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    for selected in [true, false] {
        let card = cx
            .debug_bounds(card_selector(selected))
            .expect("a theme card is drawn");
        let label = cx
            .debug_bounds(label_selector(selected))
            .expect("a theme card label is drawn");
        assert!(label.size.width > px(0.), "{label:?}");
        assert!(label.left() >= card.left(), "{label:?} in {card:?}");
        assert!(label.right() <= card.right(), "{label:?} in {card:?}");
        if selected {
            // The diamond and its gap sit left of the label.
            assert!(
                label.left() >= card.left() + px(13.),
                "{label:?} in {card:?}"
            );
            assert!(
                label.size.width <= px(super::theme_card::CARD_W + 3. - 13.),
                "{label:?}"
            );
        }
    }
}

/// The page filter shows the matching rows of every section and hides
/// the rest; picking a section in the nav ends it.
#[gpui::test]
fn filter_searches_every_section(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    cx.update(|cx| cx.set_global(ConfigPathOverride(dir.path().join("config.toml"))));
    // The filter is set before the first draw: GPUI never clears a
    // frame's debug bounds, so a row drawn in any earlier frame would
    // still be found below. "blink" is in a description on Appearance
    // (Low-end mode) and in a label on Editor (Cursor blink).
    let (page, cx) = cx.add_window_view(|_, cx| {
        let page = SettingsPage::new(WeakEntity::new_invalid(), cx);
        page.filter.update(cx, |f, cx| f.set_text("blink", cx));
        page
    });
    cx.run_until_parked();
    assert!(page.read_with(cx, |p, cx| p.filtering(cx)));
    cx.update(|window, cx| window.draw(cx).clear());
    // Matching rows of two sections are drawn...
    assert!(cx.debug_bounds("setting-row-Low-end mode").is_some());
    assert!(cx.debug_bounds("setting-row-Cursor blink").is_some());
    // ...the others are not, in the open section or elsewhere.
    assert!(cx.debug_bounds("setting-row-UI size").is_none());
    assert!(cx.debug_bounds("setting-row-Word wrap").is_none());
    // Theme cards are not drawn while filtering.
    assert!(cx.debug_bounds("theme-card-label").is_none());
    assert!(cx.debug_bounds("theme-card-label-selected").is_none());
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

/// Steppers show "13" for the UI size (whole steps, whole value) and keep
/// one decimal where the step is fractional.
#[test]
fn steppers_show_whole_numbers_for_whole_steps() {
    use super::rows::shown_value;
    let steps = |step: f32, int: bool| Steps {
        step,
        min: 0.,
        max: 100.,
        int,
    };
    assert_eq!(shown_value(13., steps(1., false)), "13");
    assert_eq!(shown_value(13.5, steps(1., false)), "13.5");
    assert_eq!(shown_value(1.0, steps(0.1, false)), "1.0");
    assert_eq!(shown_value(1.5, steps(0.1, false)), "1.5");
    assert_eq!(shown_value(150., steps(10., true)), "150");
}
