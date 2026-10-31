//! The built-in language packs.

use super::builtin_more::{java, lua, markdown, php, ruby, shell, yaml};
use super::steps::{
    check, codelldb, comp, go_install, langservers_extracted_check, lldb_dap, note, npm_global,
    optional, pack, pip_user, pipx, providing, python_import_check, run, rustup_component,
    with_run,
};
use super::{ComponentKind, LanguagePack, Os};

/// All built-in packs: Rust, Python, TypeScript/JavaScript, Go, C/C++, Web
/// (HTML/CSS/JSON), TOML, Shell, YAML, Markdown, Java, Lua, Ruby and PHP:
/// one for every language with a tree-sitter grammar in Arcode.
pub fn packs() -> Vec<LanguagePack> {
    vec![
        with_run(rust(), &["cargo run", "cargo test", "cargo build"]),
        with_run(
            python(),
            &["python main.py", "pytest", "python -m <module>"],
        ),
        with_run(typescript(), &["npm run <script>", "node <file>"]),
        with_run(go(), &["go run .", "go test ./...", "go build"]),
        with_run(cpp(), &["make <target>", "just <recipe>"]),
        web(),
        toml(),
        shell(),
        yaml(),
        markdown(),
        java(),
        lua(),
        ruby(),
        php(),
    ]
}

fn rust() -> LanguagePack {
    pack(
        "rust",
        "Rust",
        &["rust"],
        vec![
            comp(
                ComponentKind::Lsp,
                "rust-analyzer",
                vec![check("rust-analyzer", &["--version"])],
                vec![rustup_component("rust-analyzer")],
                &["rustup"],
            ),
            lldb_dap(),
            codelldb(),
            comp(
                ComponentKind::Formatter,
                "rustfmt",
                vec![check("rustfmt", &["--version"])],
                vec![rustup_component("rustfmt")],
                &["rustup"],
            ),
            comp(
                ComponentKind::Linter,
                "clippy",
                vec![check("cargo", &["clippy", "--version"])],
                vec![rustup_component("clippy")],
                &["rustup"],
            ),
        ],
    )
}

fn python() -> LanguagePack {
    pack(
        "python",
        "Python",
        &["python"],
        vec![
            providing(
                comp(
                    ComponentKind::Lsp,
                    "pyright",
                    vec![check("pyright", &["--version"])],
                    vec![npm_global(&["pyright"])],
                    &["npm"],
                ),
                &["pyright-langserver"],
            ),
            optional(comp(
                ComponentKind::Lsp,
                "python-lsp-server",
                vec![check("pylsp", &["--version"])],
                vec![pipx("python-lsp-server")],
                &["pipx"],
            )),
            comp(
                ComponentKind::Debugger,
                "debugpy",
                python_import_check("debugpy"),
                pip_user("debugpy"),
                &["python"],
            ),
            comp(
                ComponentKind::Linter,
                "ruff",
                vec![check("ruff", &["--version"])],
                vec![pipx("ruff")],
                &["pipx"],
            ),
            optional(comp(
                ComponentKind::Formatter,
                "black",
                vec![check("black", &["--version"])],
                vec![pipx("black")],
                &["pipx"],
            )),
        ],
    )
}

fn typescript() -> LanguagePack {
    let languages = [
        "typescript",
        "javascript",
        "typescriptreact",
        "javascriptreact",
    ];
    pack(
        "typescript",
        "TypeScript / JavaScript",
        &languages,
        vec![
            comp(
                ComponentKind::Lsp,
                "typescript-language-server",
                vec![check("typescript-language-server", &["--version"])],
                vec![npm_global(&["typescript", "typescript-language-server"])],
                &["npm"],
            ),
            comp(
                ComponentKind::Debugger,
                "js-debug",
                Vec::new(),
                vec![note(
                    &Os::ALL,
                    "Download js-debug-dap-<version>.tar.gz from https://github.com/microsoft/vscode-js-debug/releases, \
             extract it, and point Arcode at js-debug/src/dapDebugServer.js (run with node).",
                )],
                &["node"],
            ),
            comp(
                ComponentKind::Formatter,
                "prettier",
                vec![check("prettier", &["--version"])],
                vec![npm_global(&["prettier"])],
                &["npm"],
            ),
            comp(
                ComponentKind::Linter,
                "vscode-eslint-language-server",
                vec![langservers_extracted_check()],
                vec![npm_global(&["vscode-langservers-extracted"])],
                &["npm"],
            ),
        ],
    )
}

fn go() -> LanguagePack {
    pack(
        "go",
        "Go",
        &["go", "gomod"],
        vec![
            comp(
                ComponentKind::Lsp,
                "gopls",
                vec![check("gopls", &["version"])],
                vec![go_install("golang.org/x/tools/gopls@latest")],
                &["go"],
            ),
            comp(
                ComponentKind::Debugger,
                "dlv",
                vec![check("dlv", &["version"])],
                vec![go_install("github.com/go-delve/delve/cmd/dlv@latest")],
                &["go"],
            ),
            comp(
                ComponentKind::Formatter,
                "gofmt",
                vec![check("gofmt", &["-l"])],
                vec![note(
                    &Os::ALL,
                    "gofmt ships with Go: install Go from https://go.dev/dl/ and make sure its bin directory is on PATH.",
                )],
                &["go"],
            ),
        ],
    )
}

fn cpp() -> LanguagePack {
    pack(
        "cpp",
        "C / C++",
        &["c", "cpp", "objective-c", "objective-cpp"],
        vec![
            comp(
                ComponentKind::Lsp,
                "clangd",
                vec![check("clangd", &["--version"])],
                vec![
                    note(
                        &[Os::Linux],
                        "Install clangd from your distribution: `sudo apt install clangd` (Debian/Ubuntu), \
                 `sudo dnf install clang-tools-extra` (Fedora) or `sudo pacman -S clang` (Arch).",
                    ),
                    note(
                        &[Os::MacOs],
                        "Install the Xcode Command Line Tools (`xcode-select --install`), or LLVM with \
                 Homebrew (`brew install llvm`) and add its bin directory to PATH.",
                    ),
                    note(
                        &[Os::Windows],
                        "Install LLVM (`winget install LLVM.LLVM`) and add C:\\Program Files\\LLVM\\bin to PATH.",
                    ),
                ],
                &[],
            ),
            lldb_dap(),
            codelldb(),
            comp(
                ComponentKind::Formatter,
                "clang-format",
                vec![check("clang-format", &["--version"])],
                vec![pipx("clang-format")],
                &["pipx"],
            ),
        ],
    )
}

fn web() -> LanguagePack {
    pack(
        "web",
        "Web (HTML, CSS, JSON)",
        &["html", "css", "scss", "less", "json", "jsonc"],
        vec![
            providing(
                comp(
                    ComponentKind::Lsp,
                    "vscode-langservers-extracted",
                    vec![langservers_extracted_check()],
                    vec![npm_global(&["vscode-langservers-extracted"])],
                    &["npm"],
                ),
                &[
                    "vscode-json-language-server",
                    "vscode-html-language-server",
                    "vscode-css-language-server",
                ],
            ),
            comp(
                ComponentKind::Formatter,
                "prettier",
                vec![check("prettier", &["--version"])],
                vec![npm_global(&["prettier"])],
                &["npm"],
            ),
        ],
    )
}

fn toml() -> LanguagePack {
    pack(
        "toml",
        "TOML",
        &["toml"],
        vec![comp(
            ComponentKind::Lsp,
            "taplo",
            vec![check("taplo", &["--version"])],
            vec![run(
                &Os::ALL,
                "Build and install taplo with cargo (downloads crates from crates.io).",
                "cargo",
                &["install", "taplo-cli", "--locked", "--features", "lsp"],
            )],
            &["cargo"],
        )],
    )
}
