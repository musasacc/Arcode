use super::rustup::{ProxyCheck, check_proxy, is_proxied, proxy_target};
use super::*;

fn hint(command: &str, os: Os, path: &[&str]) -> Option<String> {
    install_hint(command, os, &|program| path.contains(&program)).map(|h| h.display())
}

#[test]
fn npm_servers_need_npm() {
    let npm = &["npm"][..];
    for (command, line) in [
        (
            "typescript-language-server",
            "npm install -g typescript typescript-language-server",
        ),
        (
            "vscode-css-language-server",
            "npm install -g vscode-langservers-extracted",
        ),
        (
            "vscode-json-language-server",
            "npm install -g vscode-langservers-extracted",
        ),
        (
            "bash-language-server",
            "npm install -g bash-language-server",
        ),
        (
            "yaml-language-server",
            "npm install -g yaml-language-server",
        ),
        ("intelephense", "npm install -g intelephense"),
    ] {
        assert_eq!(hint(command, Os::Linux, npm).as_deref(), Some(line));
        // Without npm: the homepage.
        let link = install_hint(command, Os::Linux, &|_| false).unwrap();
        assert!(matches!(link, InstallHint::Link(url) if url.starts_with("https://")));
    }
}

#[test]
fn pyright_prefers_npm_then_pipx_then_pip() {
    let pyright = |path: &[&str], os| hint("pyright-langserver", os, path);
    assert_eq!(
        pyright(&["npm", "pipx", "python3"], Os::Linux).as_deref(),
        Some("npm install -g pyright")
    );
    assert_eq!(
        pyright(&["pipx", "python3"], Os::MacOs).as_deref(),
        Some("pipx install pyright")
    );
    assert_eq!(
        pyright(&["python3"], Os::Linux).as_deref(),
        Some("python3 -m pip install --user pyright")
    );
    assert_eq!(
        pyright(&["python"], Os::Windows).as_deref(),
        Some("python -m pip install --user pyright")
    );
    assert_eq!(
        pyright(&[], Os::Linux).as_deref(),
        Some("https://github.com/microsoft/pyright")
    );
}

#[test]
fn toolchain_servers() {
    assert_eq!(
        hint("rust-analyzer", Os::Linux, &["rustup"]).as_deref(),
        Some("rustup component add rust-analyzer")
    );
    assert_eq!(
        hint("gopls", Os::Windows, &["go"]).as_deref(),
        Some("go install golang.org/x/tools/gopls@latest")
    );
    assert_eq!(
        hint("taplo", Os::MacOs, &["cargo"]).as_deref(),
        Some("cargo install taplo-cli --locked --features lsp")
    );
    assert_eq!(
        hint("ruby-lsp", Os::Linux, &["gem"]).as_deref(),
        Some("gem install ruby-lsp")
    );
    assert!(
        hint("rust-analyzer", Os::Linux, &["npm"])
            .unwrap()
            .starts_with("https://")
    );
}

#[test]
fn native_servers_use_the_package_manager_found() {
    let clangd = |path: &[&str], os| hint("clangd", os, path);
    assert_eq!(
        clangd(&["apt-get", "sudo"], Os::Linux).as_deref(),
        Some("sudo apt-get install clangd")
    );
    // Without sudo (e.g. root in a container) the manager runs directly.
    assert_eq!(
        clangd(&["dnf"], Os::Linux).as_deref(),
        Some("dnf install clang-tools-extra")
    );
    assert_eq!(
        clangd(&["pacman", "sudo"], Os::Linux).as_deref(),
        Some("sudo pacman -S clang")
    );
    // Homebrew's and winget's LLVM are not put on PATH: a link instead.
    assert_eq!(
        clangd(&["brew"], Os::MacOs).as_deref(),
        Some("https://clangd.llvm.org/installation")
    );
    assert_eq!(
        clangd(&["winget"], Os::Windows).as_deref(),
        Some("https://clangd.llvm.org/installation")
    );
    // A manager of another system does not count.
    assert!(
        clangd(&["winget"], Os::Linux)
            .unwrap()
            .starts_with("https://")
    );
    assert_eq!(
        hint("marksman", Os::MacOs, &["brew"]).as_deref(),
        Some("brew install marksman")
    );
    assert!(
        hint("marksman", Os::Linux, &["apt-get", "sudo"])
            .unwrap()
            .starts_with("https://")
    );
    assert_eq!(
        hint("lua-language-server", Os::Linux, &["pacman", "sudo"]).as_deref(),
        Some("sudo pacman -S lua-language-server")
    );
    assert_eq!(
        hint("lua-language-server", Os::Windows, &["winget"]).as_deref(),
        Some("winget install -e --id LuaLS.lua-language-server")
    );
}

#[test]
fn unknown_servers_have_no_hint() {
    assert_eq!(install_hint("typos-lsp", Os::Linux, &|_| true), None);
    assert_eq!(install_hint("/opt/clangd", Os::Linux, &|_| true), None);
    assert_eq!(homepage("typos-lsp"), None);
    assert_eq!(
        homepage("gopls"),
        Some("https://pkg.go.dev/golang.org/x/tools/gopls")
    );
}

#[test]
fn every_command_has_one_recipe() {
    let known: Vec<&str> = known_commands().collect();
    for command in &known {
        assert_eq!(
            known.iter().filter(|c| *c == command).count(),
            1,
            "{command}"
        );
        let link = install_hint(command, Os::Linux, &|_| false).unwrap();
        assert!(matches!(link, InstallHint::Link(url) if url.starts_with("https://")));
    }
}

#[test]
fn only_rustup_components_are_proxied() {
    assert!(is_proxied("rust-analyzer"));
    assert!(!is_proxied("gopls"));
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("gopls");
    std::fs::write(&plain, "x").unwrap();
    assert_eq!(
        check_proxy("gopls", &plain, None, rustup::WHICH_TIMEOUT),
        ProxyCheck::NotProxy
    );
    // A real rust-analyzer beside rustup is not a proxy.
    let real = dir.path().join("rust-analyzer");
    std::fs::write(&real, "a real server").unwrap();
    std::fs::write(
        dir.path()
            .join(format!("rustup{}", std::env::consts::EXE_SUFFIX)),
        "r",
    )
    .unwrap();
    assert_eq!(proxy_target(&real), None);
}

#[cfg(unix)]
mod fake_rustup {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    /// A rustup that knows no rust-analyzer (or, with `installed`, says
    /// where it is), as `which` and as the proxy.
    pub(super) fn install(dir: &Path, installed: bool) {
        let which = if installed {
            "echo /toolchains/stable/bin/$2; exit 0".to_string()
        } else {
            unknown("$2")
        };
        let body = format!(
            "#!/bin/sh\nname=$(basename \"$0\")\nif [ \"$name\" = rustup ]; then\n\
             if [ \"$1\" = which ]; then {which}; fi\necho \"rustup $*\"; exit 0\nfi\n{}\n",
            unknown("$name")
        );
        let rustup = dir.join("rustup");
        std::fs::write(&rustup, body).unwrap();
        std::fs::set_permissions(&rustup, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn unknown(binary: &str) -> String {
        format!(
            "echo \"error: Unknown binary '{binary}' in official toolchain \
             'stable-x86_64-unknown-linux-gnu'.\" >&2; exit 1"
        )
    }

    #[test]
    fn a_symlinked_proxy_without_its_component_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), false);
        let proxy = dir.path().join("rust-analyzer");
        std::os::unix::fs::symlink("rustup", &proxy).unwrap();
        assert_eq!(
            proxy_target(&proxy),
            Some(std::fs::canonicalize(dir.path().join("rustup")).unwrap())
        );
        let ProxyCheck::Missing(said) = check_proxy(
            "rust-analyzer",
            &proxy,
            Some(dir.path()),
            rustup::WHICH_TIMEOUT,
        ) else {
            panic!("not missing");
        };
        assert!(said.contains("Unknown binary 'rust-analyzer'"), "{said}");
    }

    #[test]
    fn a_hard_linked_proxy_with_its_component_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), true);
        let proxy = dir.path().join("rust-analyzer");
        std::fs::hard_link(dir.path().join("rustup"), &proxy).unwrap();
        assert_eq!(proxy_target(&proxy), Some(dir.path().join("rustup")));
        assert_eq!(
            check_proxy("rust-analyzer", &proxy, None, rustup::WHICH_TIMEOUT),
            ProxyCheck::Installed("/toolchains/stable/bin/rust-analyzer".into())
        );
    }

    #[test]
    fn a_rustup_that_hangs_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let rustup = dir.path().join("rustup");
        std::fs::write(&rustup, "#!/bin/sh\nexec sleep 5\n").unwrap();
        std::fs::set_permissions(&rustup, std::fs::Permissions::from_mode(0o755)).unwrap();
        let proxy = dir.path().join("rust-analyzer");
        std::os::unix::fs::symlink("rustup", &proxy).unwrap();
        let check = check_proxy(
            "rust-analyzer",
            &proxy,
            None,
            std::time::Duration::from_millis(200),
        );
        assert_eq!(check, ProxyCheck::Unknown);
    }
}
