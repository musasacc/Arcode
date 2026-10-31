use super::*;

#[test]
fn defaults_match_spec() {
    let config = Config::default();
    let e = &config.editor;
    assert_eq!(e.font_family, "JetBrains Mono");
    assert_eq!(
        e.font_fallbacks,
        [
            "Noto Sans CJK SC",
            "Noto Color Emoji",
            "Segoe UI Emoji",
            "Apple Color Emoji"
        ]
    );
    assert_eq!(e.font_size, 14.0);
    assert_eq!(e.line_height, 1.5);
    assert!(e.ligatures && e.insert_spaces && e.show_line_numbers);
    assert!(e.highlight_current_line && e.auto_close_brackets);
    assert!(!e.cursor_blink && !e.word_wrap && !e.format_on_save);
    assert_eq!(e.tab_size, 4);
    assert_eq!(e.keymap, Keymap::Default);

    let ui = &config.ui;
    assert_eq!(ui.font_family, None);
    assert_eq!(ui.font_size, 13.0);
    assert_eq!(ui.theme, ThemeChoice::System);
    assert_eq!(ui.dark_theme, "Arcode Ember");
    assert_eq!(ui.light_theme, "Arcode Chalk");
    assert!(!ui.low_end_mode && ui.animations && ui.show_perf_stats);
    assert_eq!(ui.animation_ms, 120);

    let t = &config.terminal;
    assert_eq!((t.shell.as_deref(), t.font_family.as_deref()), (None, None));
    assert!(t.args.is_empty());
    assert_eq!(t.font_size, None);
    assert_eq!(t.scrollback, 10_000);
}

#[test]
fn default_toml_round_trips_to_defaults() {
    let (config, warnings) = Config::from_toml_str(&Config::default_toml());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config, Config::default());
}

/// The template is written with `\n` line endings even from a checkout
/// with CRLF ones (Windows without `.gitattributes`).
#[test]
fn default_toml_uses_lf_line_endings() {
    assert!(!Config::default_toml().contains('\r'));
}

#[test]
fn default_toml_documents_every_option() {
    let template = Config::default_toml();
    for (section, keys) in known_keys() {
        for key in keys {
            assert!(
                template.contains(&format!("{key} =")),
                "`{section}.{key}` missing from default_toml()"
            );
        }
    }
}

#[test]
fn serialized_config_round_trips() {
    let mut config = Config::default();
    config.ui.font_family = Some("Inter".into());
    config.terminal.font_size = Some(12.0);
    config.lsp.get_mut("taplo").unwrap().initialization_options = Some(toml::Value::Table(
        toml::Table::from_iter([("a".to_owned(), toml::Value::Integer(1))]),
    ));
    let text = config.to_toml_string().unwrap();
    let (back, warnings) = Config::from_toml_str(&text);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, config);
}

#[test]
fn partial_config_merges_with_defaults() {
    let (config, warnings) = Config::from_toml_str("[editor]\nfont_size = 16\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.editor.font_size, 16.0);
    let mut expected = Config::default();
    expected.editor.font_size = 16.0;
    assert_eq!(config, expected);
}

#[test]
fn invalid_types_warn_and_keep_defaults() {
    let source = r#"
        [editor]
        font_size = "huge"
        tab_size = -3
        keymap = "emacs"
        ligatures = false
        [ui]
        theme = 7
        animation_ms = 99999
        [terminal]
        scrollback = "lots"
    "#;
    let (config, warnings) = Config::from_toml_str(source);
    assert_eq!(config.editor.font_size, 14.0);
    assert_eq!(config.editor.tab_size, 4);
    assert_eq!(config.editor.keymap, Keymap::Default);
    assert!(!config.editor.ligatures, "valid keys still apply");
    assert_eq!(config.ui.theme, ThemeChoice::System);
    assert_eq!(config.ui.animation_ms, 120);
    assert_eq!(config.terminal.scrollback, 10_000);
    for key in [
        "editor.font_size",
        "editor.tab_size",
        "editor.keymap",
        "ui.theme",
        "ui.animation_ms",
        "terminal.scrollback",
    ] {
        assert!(
            warnings.iter().any(|w| w.contains(key)),
            "no warning for {key}: {warnings:?}"
        );
    }
}

#[test]
fn out_of_range_values_are_reset() {
    let source = "[editor]\nfont_size = 0.0\nline_height = 9\ntab_size = 0\n\
                  [terminal]\nfont_size = -1.0\nscrollback = 99999999\n";
    let (config, warnings) = Config::from_toml_str(source);
    assert_eq!(config.editor.font_size, 14.0);
    assert_eq!(config.editor.line_height, 1.5);
    assert_eq!(config.editor.tab_size, 4);
    assert_eq!(config.terminal.font_size, None);
    assert_eq!(config.terminal.scrollback, TerminalSettings::MAX_SCROLLBACK);
    assert_eq!(warnings.len(), 5, "{warnings:?}");
}

#[test]
fn unknown_keys_and_sections_are_warnings() {
    let (config, warnings) =
        Config::from_toml_str("colour = 1\n[editor]\nfont_sise = 3\n[plugins]\nx = 1\n");
    assert_eq!(config, Config::default());
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("editor.font_sise")));
}

#[test]
fn syntax_error_yields_defaults_and_warning() {
    let (config, warnings) = Config::from_toml_str("[editor\nfont_size = ");
    assert_eq!(config, Config::default());
    assert_eq!(warnings.len(), 1);
    assert!(!warnings[0].contains('\n'));
    assert!(Config::is_syntax_warning(&warnings[0]));
    let (_, warnings) = Config::from_toml_str("editor = 5\n");
    assert!(!Config::is_syntax_warning(&warnings[0]));
}

#[test]
fn section_of_wrong_type_is_a_warning() {
    let (config, warnings) = Config::from_toml_str("editor = 5\nlsp = []\n");
    assert_eq!(config, Config::default());
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn theme_choice_parsing() {
    let (config, _) = Config::from_toml_str("[ui]\ntheme = \"Arcode Midnight\"");
    assert_eq!(
        config.ui.theme,
        ThemeChoice::Named("Arcode Midnight".into())
    );
    assert_eq!(ThemeChoice::parse("DARK"), ThemeChoice::Dark);
    assert_eq!(ThemeChoice::parse(" light "), ThemeChoice::Light);
    assert_eq!(ThemeChoice::parse(""), ThemeChoice::System);
    assert_eq!(ThemeChoice::Named("X".into()).to_string(), "X");
    let (config, _) = Config::from_toml_str("[editor]\nkeymap = \"vim\"");
    assert_eq!(config.editor.keymap, Keymap::Vim);
    assert!(!config.editor.vim_mode);
    let (config, warnings) = Config::from_toml_str("[editor]\nvim_mode = true");
    assert!(config.editor.vim_mode, "{warnings:?}");
}

#[test]
fn language_and_lsp_defaults() {
    let config = Config::default();
    let rust = config.servers_for_language("rust");
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].name, "rust-analyzer");
    assert_eq!(rust[0].settings.root_markers, ["Cargo.toml"]);
    for lang in ["typescript", "javascript", "tsx"] {
        assert_eq!(
            config.servers_for_language(lang)[0].name,
            "typescript-language-server"
        );
    }
    let expected = [
        ("python", "pyright-langserver"),
        ("go", "gopls"),
        ("c", "clangd"),
        ("cpp", "clangd"),
        ("toml", "taplo"),
        ("json", "vscode-json-language-server"),
    ];
    for (lang, server) in expected {
        assert_eq!(config.servers_for_language(lang)[0].name, server);
    }
    assert_eq!(config.servers_for_language("Rust")[0].name, "rust-analyzer");
    assert!(config.servers_for_language("cobol").is_empty());
    let ts = &config.lsp["typescript-language-server"];
    assert_eq!(ts.args, ["--stdio"]);
    assert_eq!(ts.root_markers, ["package.json", "tsconfig.json"]);
    assert_eq!(config.lsp["clangd"].root_markers.len(), 2);
}

#[test]
fn user_language_and_lsp_entries_override_and_extend() {
    let source = r#"
        [language.python]
        servers = ["pyright-langserver", "ruff"]
        tab_size = 2
        [language.zig]
        servers = ["zls"]
        insert_spaces = false
        [lsp.rust-analyzer]
        args = ["--log-file", "ra.log"]
        env = { RA_LOG = "info" }
        initialization_options = { cargo = { features = "all" } }
        [lsp.ruff]
        args = ["server"]
    "#;
    let (config, warnings) = Config::from_toml_str(source);
    assert!(warnings.is_empty(), "{warnings:?}");

    let py = config.servers_for_language("python");
    assert_eq!(py.len(), 2);
    assert_eq!(py[1].name, "ruff");
    assert_eq!(py[1].settings.command, "ruff", "command defaults to name");
    assert_eq!(py[1].settings.args, ["server"]);
    assert_eq!(config.effective_tab_size("python"), 2);
    assert_eq!(config.effective_tab_size("rust"), 4);
    assert_eq!(config.effective_tab_size("unknown"), 4);
    assert!(!config.effective_insert_spaces("zig"));
    assert!(config.effective_insert_spaces("python"));

    // zls has no [lsp] table: it is synthesized from the name.
    let zig = config.servers_for_language("zig");
    assert_eq!(zig[0].settings, LspServerSettings::new("zls"));

    // Field-level merge keeps the built-in root markers.
    let ra = &config.lsp["rust-analyzer"];
    assert_eq!(ra.command, "rust-analyzer");
    assert_eq!(ra.root_markers, ["Cargo.toml"]);
    assert_eq!(ra.args, ["--log-file", "ra.log"]);
    assert_eq!(ra.env["RA_LOG"], "info");
    let opts = ra.initialization_options.as_ref().unwrap();
    assert_eq!(opts["cargo"]["features"].as_str(), Some("all"));

    // Untouched built-ins survive.
    assert_eq!(config.lsp["gopls"].root_markers, ["go.mod"]);
}

#[test]
fn invalid_language_entries_warn() {
    let source = "[language.rust]\ntab_size = 0\nservers = \"ra\"\n\
                  [lsp.gopls]\ncommand = \"\"\n[language]\nbad = 3\n";
    let (config, warnings) = Config::from_toml_str(source);
    assert_eq!(config.effective_tab_size("rust"), 4);
    assert_eq!(config.servers_for_language("rust")[0].name, "rust-analyzer");
    assert_eq!(config.lsp["gopls"].command, "gopls");
    assert_eq!(warnings.len(), 4, "{warnings:?}");
}

#[test]
fn derived_helpers() {
    let mut config = Config::default();
    assert_eq!(config.terminal_font_family(), "JetBrains Mono");
    assert_eq!(config.terminal_font_size(), 14.0);
    config.terminal.font_family = Some("Hack".into());
    config.terminal.font_size = Some(11.0);
    assert_eq!(config.terminal_font_family(), "Hack");
    assert_eq!(config.terminal_font_size(), 11.0);

    assert_eq!(config.ui.effective_animation_ms(), 120);
    config.ui.low_end_mode = true;
    assert_eq!(config.ui.effective_animation_ms(), 0);

    config.editor.font_fallbacks.push("JetBrains Mono".into());
    let stack = config.editor.font_stack();
    assert_eq!(stack[0], "JetBrains Mono");
    assert_eq!(stack.len(), 5, "duplicates removed");
}

#[test]
fn load_from_missing_file_is_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (config, warnings) = Config::load_from(&dir.path().join("nope.toml"));
    assert_eq!(config, Config::default());
    assert!(warnings.is_empty());
}

#[test]
fn load_from_file_prefixes_warnings_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[ui]\nfont_size = 15\nnope = 1\n").unwrap();
    let (config, warnings) = Config::load_from(&path);
    assert_eq!(config.ui.font_size, 15.0);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(&path.display().to_string()));
}

#[test]
fn load_from_directory_warns() {
    let dir = tempfile::tempdir().unwrap();
    let (config, warnings) = Config::load_from(dir.path());
    assert_eq!(config, Config::default());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn write_default_if_missing_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/config.toml");
    assert!(Config::write_default_if_missing(&path).unwrap());
    std::fs::write(&path, "[editor]\nfont_size = 20\n").unwrap();
    assert!(!Config::write_default_if_missing(&path).unwrap());
    assert_eq!(Config::load_from(&path).0.editor.font_size, 20.0);
}

#[test]
fn large_file_cutoffs() {
    let e = Config::default().editor;
    assert_eq!(e.large_file_bytes, 4 * 1024 * 1024);
    assert_eq!(e.large_file_lines, 50_000);
    let (config, warnings) =
        Config::from_toml_str("[editor]\nlarge_file_bytes = 1000\nlarge_file_lines = 20\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.editor.large_file_bytes, 1000);
    assert_eq!(config.editor.large_file_lines, 20);
}

#[test]
fn hover_settings_parse_and_cap() {
    let config = Config::default();
    assert!(config.editor.hover && config.editor.occurrence_highlight);
    assert_eq!(config.editor.hover_delay_ms, 300);
    let (config, warnings) = Config::from_toml_str(
        "[editor]\nhover = false\nhover_delay_ms = 99999\noccurrence_highlight = false\n",
    );
    assert!(!config.editor.hover && !config.editor.occurrence_highlight);
    assert_eq!(config.editor.hover_delay_ms, 5000);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[test]
fn language_feature_settings_parse_and_cap() {
    let e = Config::default().editor;
    assert!(e.inlay_hints && e.code_action_lightbulb && !e.format_on_save);
    assert_eq!(e.format_on_save_timeout_ms, 1500);
    let (config, warnings) = Config::from_toml_str(
        "[editor]\ninlay_hints = false\ncode_action_lightbulb = false\nformat_on_save = true\nformat_on_save_timeout_ms = 60000\n",
    );
    let e = config.editor;
    assert!(!e.inlay_hints && !e.code_action_lightbulb && e.format_on_save);
    assert_eq!(e.format_on_save_timeout_ms, 10_000);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}
