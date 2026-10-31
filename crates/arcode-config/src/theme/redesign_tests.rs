//! Tests of the Ember/Chalk redesign tokens (docs/DESIGN.md sections 2
//! and 8, 7 Step 1): the design values of the two themes and how the five
//! new `[ui]` keys are derived for older theme files.

use super::*;

#[test]
fn new_keys_are_derived_from_the_theme_own_colors() {
    let mut registry = ThemeRegistry::builtin();
    let ember = registry.get("Arcode Ember").unwrap().clone();
    // A table with only `title_bar` gets `canvas == title_bar`.
    registry
        .add_user_theme("name = \"T\"\n[ui]\ntitle_bar = \"#123456\"", None)
        .unwrap();
    let t = registry.get("T").unwrap();
    assert_eq!(t.ui.canvas, Color::rgb(0x12, 0x34, 0x56));
    assert_eq!(t.ui.pill, ember.ui.pill, "no source set: inherited");
    // A table with nothing gets the base's values.
    registry.add_user_theme("name = \"N\"", None).unwrap();
    assert_eq!(registry.get("N").unwrap().ui, ember.ui);
    // Each derivation rule.
    let source = r##"
        name = "Old"
        [ui]
        background = "#000000"
        surface_raised = "#0a0b0c"
        text_muted = "#c8c8c8"
        border = "#646464"
        line_highlight = "#010203"
        warning = "#c86400"
    "##;
    registry.add_user_theme(source, None).unwrap();
    let old = &registry.get("Old").unwrap().ui;
    assert_eq!(old.canvas, ember.ui.canvas, "title_bar not set");
    assert_eq!(old.pill, Color::rgb(0x0a, 0x0b, 0x0c));
    assert_eq!(old.border_strong, Color::rgb(150, 150, 150));
    assert_eq!(old.selection_inactive, Color::rgb(1, 2, 3));
    assert_eq!(old.paused_line, Color::rgb(28, 14, 0));
    // A key the table sets always wins over derivation.
    registry
        .add_user_theme(
            "name = \"S\"\n[ui]\ntitle_bar = \"#111111\"\ncanvas = \"#222222\"",
            None,
        )
        .unwrap();
    assert_eq!(
        registry.get("S").unwrap().ui.canvas,
        Color::rgb(0x22, 0x22, 0x22)
    );
}

#[test]
fn ember_and_chalk_match_the_design_tokens() {
    let registry = ThemeRegistry::builtin();
    let hex = |name: &str, key: &str| registry.get(name).unwrap().ui.get(key).unwrap().to_hex();
    for (key, ember, chalk) in [
        ("canvas", "#0b0a09", "#dbd8d2"),
        ("background", "#25201b", "#faf9f6"),
        ("surface_raised", "#2e2822", "#ffffff"),
        ("pill", "#3a322a", "#ebe7e0"),
        ("border_strong", "#857869", "#7a7064"),
        ("selection_inactive", "#353029", "#e7e3dc"),
        ("paused_line", "#3c2e1c", "#f7e8cc"),
        ("accent", "#f0a24a", "#8a5512"),
    ] {
        assert_eq!(hex("Arcode Ember", key), ember, "Ember {key}");
        assert_eq!(hex("Arcode Chalk", key), chalk, "Chalk {key}");
    }
    let ember = registry.get("Arcode Ember").unwrap();
    assert!(ember.syntax_style("comment").unwrap().italic);
    assert!(ember.syntax_style("label").unwrap().italic);
    // Every chrome color is opaque: nothing in the new themes stacks alpha.
    for name in ["Arcode Ember", "Arcode Chalk"] {
        let ui = &registry.get(name).unwrap().ui;
        for key in UiColors::FIELDS {
            assert_eq!(ui.get(key).unwrap().a, 255, "{name}: {key} is translucent");
        }
    }
}
