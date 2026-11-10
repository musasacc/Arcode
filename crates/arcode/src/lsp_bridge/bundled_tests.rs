use std::fs;

use arcode_project::bundled::MANIFEST_FILE;

use super::*;

const MANIFEST: &str = r#"
schema = 1
[node]
version = "24.0.0"
license = "MIT"
homepage = "https://nodejs.org"
platforms = {}
[[server]]
name = "pyright"
version = "1.1.400"
languages = ["python"]
license = "MIT"
homepage = "https://example.org"
source = "npm"
packages = ["pyright@1.1.400"]
commands = [{ name = "pyright-langserver", node = "lib/npm/node_modules/pyright/langserver.index.js" }]
[[server]]
name = "rust-analyzer"
version = "2026-01-01"
languages = ["rust"]
license = "MIT"
homepage = "https://example.org"
source = "archive"
commands = [{ name = "rust-analyzer", exec = "bin/rust-analyzer" }]
"#;

fn bundle() -> (tempfile::TempDir, Bundle) {
    let dir = tempfile::tempdir().unwrap();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    for file in [
        format!("node/bin/node{exe}"),
        format!("node/node{exe}"),
        "lib/npm/node_modules/pyright/langserver.index.js".into(),
        format!("bin/rust-analyzer{exe}"),
    ] {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    fs::write(dir.path().join(MANIFEST_FILE), MANIFEST).unwrap();
    let bundle = Bundle::load(dir.path()).unwrap().unwrap();
    (dir, bundle)
}

fn spec(name: &str, command: &str) -> ServerSpec {
    ServerSpec {
        args: vec!["--stdio".into()],
        ..ServerSpec::new(name, command)
    }
}

/// `installed` are on `PATH`; `placeholders` are rustup placeholders.
fn finder<'a>(installed: &'a [&str], placeholders: &'a [&str]) -> impl Fn(&str) -> Found + 'a {
    move |command| {
        if installed.contains(&command) {
            Found::Installed(PathBuf::from("/usr/bin").join(command))
        } else if placeholders.contains(&command) {
            Found::Placeholder(format!(
                "{command} is not installed in the Rust toolchain stable: \
                 run `rustup component add {command}`"
            ))
        } else {
            Found::Missing
        }
    }
}

#[test]
fn missing_servers_run_from_the_bundle() {
    let (dir, bundle) = bundle();
    let mut servers = [
        spec("pyright-langserver", "pyright-langserver"),
        spec("rust-analyzer", "rust-analyzer"),
        spec("custom", "/opt/rust-analyzer"),
        spec("typos", "typos-lsp"),
    ];
    // rust-analyzer is installed by the user; pyright is not.
    let find = finder(&["rust-analyzer"], &[]);
    for server in &mut servers {
        assert_eq!(prepare(server, Some(&bundle), &find), None);
        assert_eq!(server.missing, None);
    }
    let node = bundle.node(arcode_project::packs::Os::current());
    assert_eq!(servers[0].command, node.to_string_lossy());
    assert_eq!(
        servers[0].args,
        [
            dir.path()
                .join("lib/npm/node_modules/pyright/langserver.index.js")
                .to_string_lossy()
                .into_owned(),
            "--stdio".to_string()
        ]
    );
    assert_eq!(servers[1].command, "rust-analyzer");
    assert_eq!(servers[2].command, "/opt/rust-analyzer");
    assert_eq!(servers[3].command, "typos-lsp");
    // Without a bundle nothing changes.
    let mut plain = spec("pyright-langserver", "pyright-langserver");
    prepare(&mut plain, None, &finder(&[], &[]));
    assert_eq!(plain.command, "pyright-langserver");
    assert_eq!(plain.missing, None);
}

#[test]
fn a_rustup_placeholder_falls_back_to_the_bundle() {
    let (dir, bundle) = bundle();
    let mut ra = spec("rust-analyzer", "rust-analyzer");
    assert_eq!(
        prepare(&mut ra, Some(&bundle), &finder(&[], &["rust-analyzer"])),
        None
    );
    let exe = if cfg!(windows) { ".exe" } else { "" };
    assert_eq!(
        PathBuf::from(&ra.command),
        dir.path().join(format!("bin/rust-analyzer{exe}"))
    );
    assert_eq!(ra.missing, None);
}

#[test]
fn a_rustup_placeholder_without_a_bundle_is_not_installed() {
    let mut ra = spec("rust-analyzer", "rust-analyzer");
    assert_eq!(
        prepare(&mut ra, None, &finder(&[], &["rust-analyzer"])),
        None
    );
    assert_eq!(ra.command, "rust-analyzer");
    let reason = ra.missing.expect("marked missing");
    assert!(arcode_lsp::is_not_installed(&reason), "{reason}");
    assert!(reason.contains("rustup component add rust-analyzer"));
    // A configured path runs as given, placeholder or not.
    let mut configured = spec("rust-analyzer", "/home/me/.cargo/bin/rust-analyzer");
    prepare(&mut configured, None, &|_| {
        panic!("a path is not looked up")
    });
    assert_eq!(configured.missing, None);
    // Other servers are never looked up without a bundle.
    let mut gopls = spec("gopls", "gopls");
    prepare(&mut gopls, None, &|_| panic!("gopls is not looked up"));
}

#[cfg(unix)]
#[test]
fn a_fake_rustup_proxy_is_a_placeholder() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let rustup = dir.path().join("rustup");
    fs::write(
        &rustup,
        "#!/bin/sh\necho \"error: Unknown binary '$2' in official toolchain \
         '1.97.0-x86_64-unknown-linux-gnu'.\" >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&rustup, fs::Permissions::from_mode(0o755)).unwrap();
    let proxy = dir.path().join("rust-analyzer");
    std::os::unix::fs::symlink("rustup", &proxy).unwrap();
    let Found::Placeholder(reason) =
        classify("rust-analyzer", Some(proxy.clone()), Some(dir.path()))
    else {
        panic!("not a placeholder");
    };
    assert!(arcode_lsp::is_not_installed(&reason), "{reason}");
    assert!(
        reason.contains("1.97.0-x86_64-unknown-linux-gnu"),
        "{reason}"
    );
    assert!(reason.contains("rustup component add rust-analyzer"));
    // The same file under another name is just a program.
    assert_eq!(
        classify("gopls", Some(proxy.clone()), None),
        Found::Installed(proxy)
    );
    assert_eq!(classify("gopls", None, None), Found::Missing);
}

// A project pinned to a toolchain that is not installed: rustup's answer
// is not about the component, so the proxy runs (and rustup can install
// the toolchain) rather than being reported as a missing component.
#[cfg(unix)]
#[test]
fn a_missing_toolchain_is_not_a_missing_component() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let rustup = dir.path().join("rustup");
    fs::write(
        &rustup,
        "#!/bin/sh\necho \"error: toolchain 'nightly-2020-01-01-x86_64-unknown-linux-gnu' \
         is not installed\" >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&rustup, fs::Permissions::from_mode(0o755)).unwrap();
    let proxy = dir.path().join("rust-analyzer");
    std::os::unix::fs::symlink("rustup", &proxy).unwrap();
    assert_eq!(
        classify("rust-analyzer", Some(proxy.clone()), Some(dir.path())),
        Found::Installed(proxy)
    );
}

#[test]
fn the_hook_keeps_notes_per_server() {
    let notes = RuntimeNotes::default();
    notes
        .lock()
        .unwrap()
        .insert("jdtls".into(), "needs Java".into());
    let hook = launch_hook(notes.clone());
    // Unit tests see no bundle: nothing changes, and a server that
    // starts without a problem drops its old note.
    let mut jdtls = spec("jdtls", "jdtls");
    hook(&mut jdtls, Path::new("."));
    assert_eq!(jdtls.command, "jdtls");
    assert!(notes.lock().unwrap().is_empty());
}
