//! User theme loading: `extends` across files, cycles and nested syntax keys.

use std::path::Path;

use arcode_config::{Color, ThemeRegistry};

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

#[test]
fn extends_chain_across_files_in_any_order() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.toml", "name = \"A\"\nextends = \"B\"\n");
    write(
        dir.path(),
        "b.toml",
        "name = \"B\"\nextends = \"c\"\n[ui]\ntext = \"#eeeeee\"\n",
    );
    write(
        dir.path(),
        "c.toml",
        "name = \"C\"\nappearance = \"light\"\n[ui]\naccent = \"#123456\"\n",
    );
    let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
    assert!(warnings.is_empty(), "{warnings:?}");
    let a = registry.get("A").unwrap();
    assert_eq!(a.ui.accent, Color::rgb(0x12, 0x34, 0x56));
    assert_eq!(a.ui.text, Color::rgb(0xee, 0xee, 0xee));
    assert!(
        !a.appearance.is_dark(),
        "appearance inherited through chain"
    );
    let light = registry.get("Arcode Chalk").unwrap();
    assert_eq!(a.ui.background, light.ui.background);
    // Display order still follows file order.
    assert_eq!(&registry.names()[5..], ["A", "B", "C"]);
}

#[test]
fn extends_cycle_warns_and_falls_back_to_builtin() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "x.toml",
        "name = \"X\"\nextends = \"Y\"\n[ui]\naccent = \"#111111\"\n",
    );
    write(
        dir.path(),
        "y.toml",
        "name = \"Y\"\nextends = \"X\"\n[ui]\ntext = \"#fafafa\"\n",
    );
    let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("cycle"), "{warnings:?}");
    assert!(warnings[0].contains("y.toml"), "{warnings:?}");
    let dark = registry.get("Arcode Ember").unwrap();
    // Y broke the cycle (inherits Arcode Ember); X extends Y.
    let y = registry.get("Y").unwrap();
    assert_eq!(y.ui.accent, dark.ui.accent);
    let x = registry.get("X").unwrap();
    assert_eq!(x.ui.accent, Color::rgb(0x11, 0x11, 0x11));
    assert_eq!(x.ui.text, Color::rgb(0xfa, 0xfa, 0xfa));
}

#[test]
fn self_named_extends_uses_builtin_and_broken_target_warns() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "dark.toml",
        "name = \"Arcode Dark\"\nextends = \"Arcode Midnight\"\n",
    );
    write(
        dir.path(),
        "mine.toml",
        "name = \"Mine\"\nextends = \"Arcode Dark\"\n[ui]\naccent = \"#abcdef\"\n",
    );
    write(dir.path(), "broken.toml", "name = [");
    write(
        dir.path(),
        "child.toml",
        "name = \"Child\"\nextends = \"broken\"\n",
    );
    write(dir.path(), "odd.toml", "name = \"Odd\"\nextends = 3\n");
    let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
    let midnight = registry.get("Arcode Midnight").unwrap().clone();
    // The user replacement of Arcode Dark extends Midnight...
    assert_eq!(registry.get("Arcode Dark").unwrap().ui, midnight.ui);
    // ...and a theme extending "Arcode Dark" sees that replacement.
    let mine = registry.get("Mine").unwrap();
    assert_eq!(mine.ui.background, midnight.ui.background);
    assert_eq!(mine.ui.accent, Color::rgb(0xab, 0xcd, 0xef));
    // broken.toml: parse error; child.toml: unknown theme; odd.toml: not a string.
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("broken.toml")));
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("child.toml") && w.contains("unknown theme"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("odd.toml") && w.contains("extends"))
    );
}

#[test]
fn nested_syntax_tables_flatten_into_dotted_captures() {
    let mut registry = ThemeRegistry::builtin();
    let dark = registry.get("Arcode Ember").unwrap().clone();
    let source = r##"
        name = "Nested"
        [syntax]
        type.builtin = { color = "#00ff00", italic = true }
        keyword.control.return = "#0000ff"
        [syntax.string]
        color = "#ff0000"
        bold = true
        escape = "#00ffff"
        regex = { underline = true }
    "##;
    let (_, warnings) = registry.add_user_theme(source, None).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let theme = registry.get("Nested").unwrap();

    // `type` only has children: its own style is inherited.
    assert_eq!(theme.syntax_style("type"), dark.syntax_style("type"));
    let builtin = theme.syntax_style("type.builtin").unwrap();
    assert_eq!(builtin.color, Some(Color::rgb(0, 255, 0)));
    assert!(builtin.italic && !builtin.bold);

    assert_eq!(theme.syntax_style("keyword"), dark.syntax_style("keyword"));
    assert_eq!(
        theme.syntax_color("keyword.control.return"),
        Some(Color::rgb(0, 0, 255))
    );
    assert_eq!(
        theme.syntax_color("keyword.control"),
        dark.syntax_color("keyword")
    );

    // A table mixing style keys and children styles both.
    let string = theme.syntax_style("string").unwrap();
    assert_eq!(string.color, Some(Color::rgb(255, 0, 0)));
    assert!(string.bold);
    assert_eq!(
        theme.syntax_color("string.escape"),
        Some(Color::rgb(0, 255, 255))
    );
    let regex = theme.syntax_style("string.regex").unwrap();
    assert!(regex.underline && regex.color.is_none());
}

#[test]
fn invalid_nested_syntax_values_warn_without_touching_parent() {
    let mut registry = ThemeRegistry::builtin();
    let dark = registry.get("Arcode Ember").unwrap().clone();
    let source = r##"
        name = "Bad"
        [syntax]
        function.method = 5
        function.macro = { bold = "yes" }
        string = { color = "nope", escape = "#ffffff" }
    "##;
    let (_, warnings) = registry.add_user_theme(source, None).unwrap();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("function.method")));
    assert!(warnings.iter().any(|w| w.contains("function.macro")));
    let theme = registry.get("Bad").unwrap();
    assert_eq!(
        theme.syntax_style("function"),
        dark.syntax_style("function")
    );
    assert_eq!(
        theme.syntax_style("function.method"),
        dark.syntax_style("function.method")
    );
    // The invalid `string` style is ignored, its valid child still applies.
    assert_eq!(theme.syntax_style("string"), dark.syntax_style("string"));
    assert_eq!(
        theme.syntax_color("string.escape"),
        Some(Color::rgb(255, 255, 255))
    );
}
