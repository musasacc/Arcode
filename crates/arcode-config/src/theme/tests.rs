use super::*;

/// Capture names every built-in theme must style.
const REQUIRED_SYNTAX: &[&str] = &[
    "keyword",
    "function",
    "function.method",
    "function.macro",
    "type",
    "type.builtin",
    "string",
    "string.escape",
    "comment",
    "number",
    "constant",
    "constant.builtin",
    "boolean",
    "variable",
    "variable.parameter",
    "variable.builtin",
    "property",
    "attribute",
    "operator",
    "punctuation",
    "punctuation.bracket",
    "label",
    "module",
    "namespace",
    "constructor",
    "escape",
    "tag",
];

const BUILTIN_FILES: [&str; 3] = [
    include_str!("../../themes/arcode-dark.toml"),
    include_str!("../../themes/arcode-light.toml"),
    include_str!("../../themes/arcode-midnight.toml"),
];

#[test]
fn builtins_parse_without_warnings() {
    for source in BUILTIN_FILES {
        let (theme, warnings) = Theme::from_toml_str(source, None).unwrap();
        assert!(warnings.is_empty(), "{}: {warnings:?}", theme.name);
    }
    let registry = ThemeRegistry::builtin();
    assert_eq!(registry.names(), BUILTIN_THEME_NAMES);
    assert_eq!(
        registry.get("Arcode Light").unwrap().appearance,
        Appearance::Light
    );
    assert_eq!(
        registry.get("Arcode Midnight").unwrap().appearance,
        Appearance::Dark
    );
}

#[test]
fn builtins_define_every_key_explicitly() {
    for source in BUILTIN_FILES {
        let table: toml::Table = toml::from_str(source).unwrap();
        let name = table["name"].as_str().unwrap();
        for (section, keys) in [
            ("ui", UiColors::FIELDS),
            ("terminal", &TerminalColors::FIELDS[..]),
            ("syntax", REQUIRED_SYNTAX),
        ] {
            let present = table[section].as_table().unwrap();
            for key in keys {
                assert!(
                    present.contains_key(*key),
                    "{name}: missing {section}.{key}"
                );
            }
        }
    }
}

#[test]
fn builtins_have_readable_contrast() {
    for theme in ThemeRegistry::builtin().themes() {
        let ui = &theme.ui;
        let bg = ui.background;
        let check = |what: &str, color: Color, min: f32| {
            let ratio = color.over(bg).contrast_ratio(bg);
            assert!(
                ratio >= min,
                "{}: {what} contrast {ratio:.2} < {min}",
                theme.name
            );
        };
        check("text", ui.text, 7.0);
        check("text_muted", ui.text_muted, 4.5);
        check("gutter_text", ui.gutter_text, 3.0);
        check("accent", ui.accent, 3.0);
        let on_accent = ui.accent_text.contrast_ratio(ui.accent);
        assert!(
            on_accent >= 4.5,
            "{}: accent_text {on_accent:.2}",
            theme.name
        );
        for (capture, style) in &theme.syntax {
            let color = style.color.unwrap();
            let min = if capture.starts_with("comment") {
                4.0
            } else {
                4.5
            };
            check(capture, color, min);
        }
        let term = &theme.terminal;
        let fg = term.foreground.contrast_ratio(term.background);
        assert!(fg >= 7.0, "{}: terminal fg {fg:.2}", theme.name);
    }
}

#[test]
fn syntax_style_prefix_fallback() {
    let registry = ThemeRegistry::builtin();
    let theme = registry.get("Arcode Dark").unwrap();
    let method = theme.syntax_style("function.method").unwrap();
    assert_eq!(theme.syntax_style("function.method.builtin"), Some(method));
    assert_eq!(theme.syntax_style("@function.method.builtin"), Some(method));
    assert_eq!(
        theme.syntax_style("keyword.control.return"),
        theme.syntax_style("keyword")
    );
    assert_eq!(theme.syntax_style("nonexistent.capture"), None);
    assert_eq!(theme.syntax_style(""), None);
    assert!(theme.syntax_style("comment").unwrap().italic);
    assert_eq!(
        theme.syntax_color("string.regex"),
        theme.syntax_color("string")
    );
}

#[test]
fn partial_user_theme_inherits_from_same_appearance() {
    let mut registry = ThemeRegistry::builtin();
    let source = r##"
        name = "Paper"
        appearance = "light"
        [ui]
        background = "#fffff0"
        [syntax]
        keyword = { color = "#aa0000", bold = true, underline = true }
        comment = "#777777"
        [terminal]
        bright_red = "#ff0000"
    "##;
    let (name, warnings) = registry.add_user_theme(source, None).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let light = registry.get("Arcode Light").unwrap().clone();
    let paper = registry.get(&name).unwrap();
    assert_eq!(paper.ui.background, Color::rgb(0xff, 0xff, 0xf0));
    assert_eq!(paper.ui.text, light.ui.text);
    assert_eq!(paper.ui.accent, light.ui.accent);
    let keyword = paper.syntax_style("keyword").unwrap();
    assert!(keyword.bold && keyword.underline && !keyword.italic);
    let comment = paper.syntax_style("comment").unwrap();
    assert_eq!(comment.color, Some(Color::rgb(0x77, 0x77, 0x77)));
    assert!(!comment.italic, "shorthand replaces the whole style");
    assert_eq!(paper.syntax_style("string"), light.syntax_style("string"));
    assert_eq!(paper.terminal.ansi[9], Color::rgb(255, 0, 0));
    assert_eq!(paper.terminal.foreground, light.terminal.foreground);
}

#[test]
fn user_theme_extends_named_theme() {
    let mut registry = ThemeRegistry::builtin();
    let (_, warnings) = registry
        .add_user_theme("name = \"Deep\"\nextends = \"Arcode Midnight\"\n", None)
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let deep = registry.get("deep").unwrap();
    let midnight = registry.get("Arcode Midnight").unwrap();
    assert_eq!(deep.appearance, Appearance::Dark);
    assert_eq!(deep.ui, midnight.ui);

    let (_, warnings) = registry
        .add_user_theme(
            "name = \"X\"\nextends = \"Nope\"\nappearance = \"light\"",
            None,
        )
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        registry.get("X").unwrap().ui,
        registry.get("Arcode Light").unwrap().ui
    );
}

#[test]
fn invalid_theme_values_warn() {
    let mut registry = ThemeRegistry::builtin();
    let source = r##"
        name = "Odd"
        flavour = "x"
        [ui]
        background = "blue"
        sparkle = "#fff"
        [syntax]
        keyword = { color = "#f00", wobbly = true }
        string = 5
        type = { bold = "yes" }
    "##;
    let (_, warnings) = registry.add_user_theme(source, None).unwrap();
    assert_eq!(warnings.len(), 6, "{warnings:?}");
    let odd = registry.get("Odd").unwrap();
    let dark = registry.get("Arcode Dark").unwrap();
    assert_eq!(odd.appearance, Appearance::Dark, "defaults to dark");
    assert_eq!(odd.ui.background, dark.ui.background);
    assert_eq!(odd.syntax_style("string"), dark.syntax_style("string"));
    assert_eq!(odd.syntax_color("keyword"), Some(Color::rgb(255, 0, 0)));
}

#[test]
fn strict_parsing_requires_everything() {
    let err = Theme::from_toml_str("name = \"A\"\nappearance = \"dark\"", None).unwrap_err();
    assert!(err.to_string().contains("missing ui color"), "{err}");
    let err = Theme::from_toml_str("appearance = \"dark\"", None).unwrap_err();
    assert!(err.to_string().contains("name"));
    let err = Theme::from_toml_str("name = \"A\"", None).unwrap_err();
    assert!(err.to_string().contains("appearance"));
    let err = Theme::from_toml_str("name = \"A\"\nappearance = \"dim\"", None).unwrap_err();
    assert!(err.to_string().contains("appearance"));
    assert!(Theme::from_toml_str("name = [", None).is_err());
}

#[test]
fn user_theme_can_replace_builtin() {
    let mut registry = ThemeRegistry::builtin();
    registry
        .add_user_theme("name = \"Arcode Dark\"\n[ui]\naccent = \"#ff00ff\"", None)
        .unwrap();
    assert_eq!(registry.names().len(), 3);
    let dark = registry.get("Arcode Dark").unwrap();
    assert_eq!(dark.ui.accent, Color::rgb(255, 0, 255));
}

#[test]
fn resolve_choices() {
    let mut registry = ThemeRegistry::builtin();
    registry
        .add_user_theme("name = \"Solar\"\nappearance = \"light\"", None)
        .unwrap();
    use crate::config::ThemeChoice as C;
    let resolve = |choice: C, dark: &str, light: &str, sys: bool| {
        registry.resolve(&choice, dark, light, sys).name.clone()
    };
    let (d, l) = ("Arcode Midnight", "Solar");
    assert_eq!(resolve(C::System, d, l, true), "Arcode Midnight");
    assert_eq!(resolve(C::System, d, l, false), "Solar");
    assert_eq!(resolve(C::Dark, d, l, false), "Arcode Midnight");
    assert_eq!(resolve(C::Light, d, l, true), "Solar");
    assert_eq!(resolve(C::Named("solar".into()), d, l, true), "Solar");
    // Unknown names fall back to the built-in of the relevant appearance.
    assert_eq!(resolve(C::Named("Gone".into()), d, l, true), "Arcode Dark");
    assert_eq!(
        resolve(C::Named("Gone".into()), d, l, false),
        "Arcode Light"
    );
    assert_eq!(resolve(C::Light, d, "Missing", true), "Arcode Light");
    assert_eq!(resolve(C::System, "Missing", l, true), "Arcode Dark");
}

#[test]
fn load_from_directory() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write("a-ok.toml", "name = \"Ok\"\nappearance = \"light\"\n");
    write("b-broken.toml", "name = [");
    write("c-noname.TOML", "appearance = \"dark\"\n");
    write("d-dup.toml", "name = \"Ok\"\n");
    write("notes.txt", "ignored");
    std::fs::create_dir(dir.path().join("sub.toml")).unwrap();

    let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
    assert_eq!(
        registry.names(),
        [
            "Arcode Dark",
            "Arcode Light",
            "Arcode Midnight",
            "Ok",
            "c-noname"
        ]
    );
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains("b-broken.toml"));
    assert!(warnings[1].contains("replaces"));
    // The later duplicate (no appearance) inherits from Arcode Dark.
    assert_eq!(registry.get("Ok").unwrap().appearance, Appearance::Dark);
}

#[test]
fn load_missing_or_no_dir() {
    let (registry, warnings) = ThemeRegistry::load(None);
    assert_eq!(registry.themes().len(), 3);
    assert!(warnings.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let (registry, warnings) = ThemeRegistry::load(Some(&dir.path().join("missing")));
    assert_eq!(registry.themes().len(), 3);
    assert!(warnings.is_empty());
}

#[test]
fn fallback_and_insert() {
    let mut registry = ThemeRegistry::default();
    assert_eq!(registry.fallback(Appearance::Light).name, "Arcode Light");
    let mut copy = registry.get("Arcode Light").unwrap().clone();
    copy.name = "Copy".into();
    assert!(registry.insert(copy.clone()).is_none());
    assert_eq!(
        registry.insert(copy).map(|t| t.name),
        Some("Copy".to_owned())
    );
    assert!(Appearance::Dark.is_dark() && !Appearance::Light.is_dark());
}
