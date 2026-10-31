//! Case-insensitive `[language.<id>]` and `[lsp.<name>]` merging.

use arcode_config::Config;

#[test]
fn language_ids_are_lowercased_and_merged() {
    let (config, warnings) = Config::from_toml_str(
        "[language.Python]\ninsert_spaces = false\n[language.MyLang]\ntab_size = 3\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!config.languages.contains_key("Python"));
    assert!(!config.effective_insert_spaces("python"));
    // Built-in fields of the merged entry are kept.
    assert_eq!(
        config.servers_for_language("python")[0].name,
        "pyright-langserver"
    );
    assert!(config.languages.contains_key("mylang"));
    assert_eq!(config.effective_tab_size("MYLANG"), 3);
    assert_eq!(config.effective_tab_size("mylang"), 3);
}

#[test]
fn same_language_twice_with_different_case_warns() {
    let (config, warnings) = Config::from_toml_str(
        "[language.Rust]\ntab_size = 2\n[language.rust]\ninsert_spaces = false\n",
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("same entry"), "{warnings:?}");
    assert_eq!(config.effective_tab_size("rust"), 2);
    assert!(!config.effective_insert_spaces("rust"));
}

#[test]
fn lsp_names_merge_case_insensitively_but_keep_new_names_verbatim() {
    let source = r#"
        [lsp.Rust-Analyzer]
        args = ["--verbose"]

        [lsp.MyServer]
        args = ["--stdio"]

        [language.mine]
        servers = ["myserver"]
    "#;
    let (config, warnings) = Config::from_toml_str(source);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!config.lsp.contains_key("Rust-Analyzer"));
    let ra = &config.lsp["rust-analyzer"];
    assert_eq!(ra.args, ["--verbose"]);
    assert_eq!(ra.root_markers, ["Cargo.toml"], "built-in markers kept");

    // A new server keeps its spelling (it is the default command).
    assert_eq!(config.lsp["MyServer"].command, "MyServer");
    let servers = config.servers_for_language("mine");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].settings.args, ["--stdio"]);
    assert_eq!(servers[0].settings.command, "MyServer");
    assert!(config.lsp_server("MYSERVER").is_some());
    assert!(config.lsp_server("nothing").is_none());
}
