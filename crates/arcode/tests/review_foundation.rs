//! Review tests for the Ember/Chalk foundation lane (docs/DESIGN.md 4, 7
//! Step 2.3): the UI font default is the bundled Inter now.

use arcode::prefs::pickers::{FontTarget, font_items};
use arcode_config::Config;
use arcode_ui_kit::BUNDLED_UI_FONT;

/// With `ui.font_family` unset the workbench renders in the bundled Inter
/// (`ui_font_family`), so the picker's "default" row must say so instead
/// of "System UI Font".
#[test]
fn ui_font_picker_default_row_names_inter() {
    let config = Config::default();
    assert_eq!(config.ui.font_family, None);
    let (items, values) = font_items(FontTarget::Ui, &[], &config);
    assert_eq!(values[0], "", "first row is the default");
    let default = &items[0];
    let text = format!("{} {}", default.label, default.detail);
    assert!(
        text.contains(BUNDLED_UI_FONT),
        "the unset UI font renders as {BUNDLED_UI_FONT}, but the picker's default row reads {text:?}"
    );
}
