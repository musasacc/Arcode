//! Built-in packs for Shell, YAML, Markdown, Java, Lua, Ruby and PHP.

use super::steps::{
    check, comp, gem_install, note, npm_global, npm_ls_check, optional, pack, with_run,
};
use super::{ComponentKind, LanguagePack, Os};

pub(super) fn shell() -> LanguagePack {
    with_run(
        pack(
            "shell",
            "Shell Script",
            &["shellscript"],
            vec![
                comp(
                    ComponentKind::Lsp,
                    "bash-language-server",
                    vec![check("bash-language-server", &["--version"])],
                    vec![npm_global(&["bash-language-server"])],
                    &["npm"],
                ),
                optional(comp(
                    ComponentKind::Linter,
                    "shellcheck",
                    vec![check("shellcheck", &["--version"])],
                    vec![
                        note(
                            &[Os::Linux],
                            "Install ShellCheck from your distribution (`sudo apt install shellcheck`, \
                             `sudo dnf install ShellCheck`); bash-language-server uses it when found.",
                        ),
                        note(
                            &[Os::MacOs],
                            "Install ShellCheck with Homebrew: `brew install shellcheck`.",
                        ),
                        note(
                            &[Os::Windows],
                            "Install ShellCheck: `winget install koalaman.shellcheck`.",
                        ),
                    ],
                    &[],
                )),
            ],
        ),
        &["bash <file>"],
    )
}

pub(super) fn yaml() -> LanguagePack {
    pack(
        "yaml",
        "YAML",
        &["yaml"],
        vec![comp(
            ComponentKind::Lsp,
            "yaml-language-server",
            vec![npm_ls_check("yaml-language-server")],
            vec![npm_global(&["yaml-language-server"])],
            &["npm"],
        )],
    )
}

pub(super) fn markdown() -> LanguagePack {
    pack(
        "markdown",
        "Markdown",
        &["markdown"],
        vec![comp(
            ComponentKind::Lsp,
            "marksman",
            vec![check("marksman", &["--version"])],
            vec![
                note(
                    &[Os::Linux],
                    "Download the marksman binary for your platform from \
                     https://github.com/artempyanykh/marksman/releases and put it on PATH \
                     (or `sudo snap install marksman`).",
                ),
                note(
                    &[Os::MacOs],
                    "Install marksman with Homebrew: `brew install marksman`.",
                ),
                note(
                    &[Os::Windows],
                    "Install marksman: `winget install Artempyanykh.Marksman`.",
                ),
            ],
            &[],
        )],
    )
}

pub(super) fn java() -> LanguagePack {
    with_run(
        pack(
            "java",
            "Java",
            &["java"],
            vec![comp(
                ComponentKind::Lsp,
                "jdtls",
                Vec::new(),
                vec![
                    note(
                        &[Os::Linux, Os::Windows],
                        "Install a JDK (21 or newer), download Eclipse JDT Language Server from \
                         https://download.eclipse.org/jdtls/snapshots/, extract it and put its \
                         bin/jdtls launcher on PATH.",
                    ),
                    note(
                        &[Os::MacOs],
                        "Install jdtls with Homebrew: `brew install jdtls`.",
                    ),
                ],
                &[],
            )],
        ),
        &["mvn package", "gradle build"],
    )
}

pub(super) fn lua() -> LanguagePack {
    with_run(
        pack(
            "lua",
            "Lua",
            &["lua"],
            vec![comp(
                ComponentKind::Lsp,
                "lua-language-server",
                vec![check("lua-language-server", &["--version"])],
                vec![
                    note(
                        &[Os::Linux],
                        "Install lua-language-server from your distribution (Arch: \
                         `sudo pacman -S lua-language-server`) or download a release from \
                         https://github.com/LuaLS/lua-language-server/releases and put its bin \
                         folder on PATH.",
                    ),
                    note(
                        &[Os::MacOs],
                        "Install lua-language-server with Homebrew: `brew install lua-language-server`.",
                    ),
                    note(
                        &[Os::Windows],
                        "Install lua-language-server: `winget install LuaLS.lua-language-server`.",
                    ),
                ],
                &[],
            )],
        ),
        &["lua <file>"],
    )
}

pub(super) fn ruby() -> LanguagePack {
    with_run(
        pack(
            "ruby",
            "Ruby",
            &["ruby"],
            vec![comp(
                ComponentKind::Lsp,
                "ruby-lsp",
                vec![check("ruby-lsp", &["--version"])],
                vec![gem_install("ruby-lsp")],
                &["gem"],
            )],
        ),
        &["ruby <file>", "bundle exec rake"],
    )
}

pub(super) fn php() -> LanguagePack {
    with_run(
        pack(
            "php",
            "PHP",
            &["php"],
            vec![comp(
                ComponentKind::Lsp,
                "intelephense",
                vec![npm_ls_check("intelephense")],
                vec![npm_global(&["intelephense"])],
                &["npm"],
            )],
        ),
        &["php <file>"],
    )
}
