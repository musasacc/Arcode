use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn repo_manifest() -> Manifest {
    let text = fs::read_to_string(repo_file("packaging/language-servers.toml")).unwrap();
    Manifest::parse(&text).unwrap()
}

#[test]
fn the_repository_manifest_is_valid() {
    let manifest = repo_manifest();
    assert_eq!(manifest.validate(), Vec::<String>::new());
    // Every platform has a pinned download for node and for each archive
    // server (or is explicitly unsupported).
    for platform in PLATFORMS {
        let node = &manifest.node.platforms[platform];
        assert!(node.url.starts_with("https://nodejs.org/dist/v"));
        assert_eq!(node.sha256.len(), 64);
        for server in manifest
            .servers
            .iter()
            .filter(|s| s.source == Source::Archive)
        {
            let unsupported = server.unsupported.iter().any(|u| u == platform);
            assert!(
                server.platforms.contains_key(platform) != unsupported,
                "{} {platform}",
                server.name
            );
        }
    }
}

#[test]
fn the_manifest_covers_the_builtin_servers() {
    let manifest = repo_manifest();
    for command in [
        "rust-analyzer",
        "clangd",
        "gopls",
        "pyright-langserver",
        "ruff",
        "typescript-language-server",
        "vscode-html-language-server",
        "vscode-css-language-server",
        "vscode-json-language-server",
        "yaml-language-server",
        "bash-language-server",
        "lua-language-server",
        "marksman",
        "taplo",
        "jdtls",
    ] {
        assert!(
            manifest.command(command).is_some(),
            "{command} is not bundled"
        );
    }
    let left_out: Vec<&str> = manifest
        .not_bundled
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(left_out, ["ruby-lsp", "intelephense"]);
}

#[test]
fn npm_servers_match_the_lockfile() {
    let manifest = repo_manifest();
    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo_file("packaging/npm/package.json")).unwrap())
            .unwrap();
    let lock: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_file("packaging/npm/package-lock.json")).unwrap(),
    )
    .unwrap();
    let dependencies = package["dependencies"].as_object().unwrap();
    let mut pinned = BTreeMap::new();
    for server in manifest.servers.iter().filter(|s| s.source == Source::Npm) {
        for spec in &server.packages {
            let (name, version) = npm_spec(spec).unwrap();
            pinned.insert(name.to_string(), version.to_string());
        }
    }
    let declared: BTreeMap<String, String> = dependencies
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(declared, pinned, "package.json and the manifest disagree");
    for (name, version) in &pinned {
        let locked = &lock["packages"][format!("node_modules/{name}")]["version"];
        assert_eq!(
            locked.as_str(),
            Some(version.as_str()),
            "{name} in the lockfile"
        );
    }
    // Every locked package is verified by npm.
    for (path, entry) in lock["packages"].as_object().unwrap() {
        if !path.is_empty() {
            assert!(entry["integrity"].is_string(), "{path} has no integrity");
        }
    }
}

const MINIMAL: &str = r#"
schema = 1
[node]
version = "24.0.0"
license = "MIT"
homepage = "https://nodejs.org"
licenses = [{ path = "LICENSE" }]
[node.platforms.linux-x64]
url = "https://nodejs.org/a.tar.xz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
[node.platforms.linux-arm64]
url = "https://nodejs.org/b.tar.xz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
[node.platforms.macos-arm64]
url = "https://nodejs.org/c.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
[node.platforms.macos-x64]
url = "https://nodejs.org/d.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
[node.platforms.windows-x64]
url = "https://nodejs.org/e.zip"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

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
name = "tool"
version = "2.0.0"
languages = ["toml"]
license = "MIT"
homepage = "https://example.org"
source = "archive"
install = { file = "bin/tool" }
commands = [{ name = "tool", exec = "bin/tool" }]
licenses = [{ url = "https://example.org/LICENSE", sha256 = "1111111111111111111111111111111111111111111111111111111111111111" }]
unsupported = ["linux-arm64", "macos-arm64", "macos-x64", "windows-x64"]
note = "Linux x64 only."
[server.platforms.linux-x64]
url = "https://example.org/tool.gz"
sha256 = "2222222222222222222222222222222222222222222222222222222222222222"

[[server]]
name = "jdtls"
version = "1.0.0"
languages = ["java"]
license = "EPL-2.0"
homepage = "https://example.org"
source = "archive"
install = { dir = "lib/jdtls" }
commands = [{ name = "jdtls", java = "lib/jdtls" }]
licenses = [{ path = "LICENSE" }]
unsupported = ["linux-x64", "linux-arm64", "macos-arm64", "macos-x64", "windows-x64"]
note = "test"
"#;

fn minimal() -> Manifest {
    let manifest = Manifest::parse(MINIMAL).unwrap();
    assert_eq!(manifest.validate(), Vec::<String>::new());
    manifest
}

fn problems_after(edit: impl FnOnce(&mut Manifest)) -> Vec<String> {
    let mut manifest = minimal();
    edit(&mut manifest);
    manifest.validate()
}

#[test]
fn validation_requires_pinned_versions() {
    for version in ["latest", "^1.1.400", "~1.1", "1.x", ">=1.0.0", ""] {
        let problems = problems_after(|m| m.servers[1].version = version.into());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("not pinned") || p.contains("no version")),
            "{version}: {problems:?}"
        );
    }
    let problems = problems_after(|m| m.servers[0].packages = vec!["pyright@^1.1.400".into()]);
    assert!(
        problems.iter().any(|p| p.contains("is not pinned")),
        "{problems:?}"
    );
    let problems = problems_after(|m| m.node.version = "24".into());
    assert!(
        problems.iter().any(|p| p.starts_with("node: version")),
        "{problems:?}"
    );
    // The server's own package must be in its npm packages.
    let problems = problems_after(|m| m.servers[0].version = "1.1.401".into());
    assert!(
        problems
            .iter()
            .any(|p| p.contains("do not include pyright@1.1.401"))
    );
}

#[test]
fn validation_requires_a_checked_download_per_platform() {
    let problems = problems_after(|m| {
        m.node.platforms.remove("windows-x64");
    });
    assert_eq!(problems, ["node: no download for windows-x64"]);
    let problems = problems_after(|m| m.servers[1].unsupported.retain(|p| p != "macos-x64"));
    assert_eq!(problems, ["tool: no download for macos-x64"]);
    let problems = problems_after(|m| {
        let d = m.servers[1].platforms.get_mut("linux-x64").unwrap();
        d.sha256 = "ABC".into();
        d.url = "http://example.org/tool.gz".into();
    });
    assert_eq!(
        problems,
        [
            "tool: linux-x64 URL is not https",
            "tool: linux-x64 has no valid sha256"
        ]
    );
    let problems = problems_after(|m| m.servers[1].licenses[0].sha256 = None);
    assert!(problems[0].contains("has no valid sha256"), "{problems:?}");
    let problems = problems_after(|m| m.servers[1].note = None);
    assert_eq!(problems, ["tool: unsupported platforms need a note"]);
}

#[test]
fn validation_checks_commands_and_layout() {
    let problems = problems_after(|m| {
        m.servers[1].commands[0].node = Some("lib/npm/node_modules/x.js".into());
    });
    assert!(
        problems
            .iter()
            .any(|p| p.contains("exactly one of exec, node, java"))
    );
    let problems = problems_after(|m| m.servers[1].commands[0].exec = Some("../bin/tool".into()));
    assert!(problems.iter().any(|p| p.contains("not a relative path")));
    let problems = problems_after(|m| m.servers[1].install = None);
    assert_eq!(problems, ["tool: install needs exactly one of file, dir"]);
    let problems = problems_after(|m| {
        let copy = m.servers[0].commands[0].clone();
        m.servers[1].commands.push(copy);
    });
    assert!(
        problems
            .iter()
            .any(|p| p.contains("pyright-langserver is listed twice"))
    );
}

/// `path` with `/` separators, so tests written with one layout compare
/// the same on Windows, where joined paths use backslashes.
fn slashes(path: impl AsRef<str>) -> String {
    path.as_ref().replace('\\', "/")
}

/// A bundle in a temp folder with the given files.
fn bundle_with(files: &[&str]) -> (tempfile::TempDir, Bundle) {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    fs::write(dir.path().join(MANIFEST_FILE), MINIMAL).unwrap();
    let bundle = Bundle::load(dir.path()).unwrap().unwrap();
    (dir, bundle)
}

fn context(os: Os, java: Option<&str>) -> LaunchContext {
    LaunchContext {
        os,
        arm64: false,
        java: java.map(PathBuf::from),
        java_version: None,
        state_dir: PathBuf::from("/state"),
    }
}

const PYRIGHT_FILES: [&str; 3] = [
    "node/bin/node",
    "node/node.exe",
    "lib/npm/node_modules/pyright/langserver.index.js",
];

#[test]
fn node_servers_run_with_the_bundled_node() {
    let (dir, bundle) = bundle_with(&PYRIGHT_FILES);
    let script = dir
        .path()
        .join("lib/npm/node_modules/pyright/langserver.index.js");
    let launch = bundle
        .launch("pyright-langserver", &context(Os::Linux, None))
        .unwrap();
    assert_eq!(launch.program, dir.path().join("node/bin/node"));
    assert_eq!(launch.args, [script.to_string_lossy().into_owned()]);
    assert_eq!(launch.missing_runtime, None);
    let windows = bundle
        .launch("pyright-langserver", &context(Os::Windows, None))
        .unwrap();
    assert_eq!(windows.program, dir.path().join("node/node.exe"));
    // Without its runtime the script is not usable.
    let (_dir, no_node) = bundle_with(&PYRIGHT_FILES[2..]);
    assert!(!no_node.provides("pyright-langserver", Os::Linux));
    assert_eq!(
        no_node.launch("pyright-langserver", &context(Os::Linux, None)),
        None
    );
}

#[test]
fn native_servers_get_exe_on_windows() {
    let (dir, bundle) = bundle_with(&["bin/tool", "bin/tool.exe"]);
    let linux = bundle.launch("tool", &context(Os::Linux, None)).unwrap();
    assert_eq!(
        (linux.program, linux.args),
        (dir.path().join("bin/tool"), vec![])
    );
    let windows = bundle.launch("tool", &context(Os::Windows, None)).unwrap();
    assert_eq!(windows.program, dir.path().join("bin/tool.exe"));
    // A command the manifest lists but the bundle lacks (left out when
    // it was fetched) is not provided.
    let (_dir, empty) = bundle_with(&[]);
    assert!(!empty.provides("tool", Os::Linux));
    assert!(!empty.provides("unknown", Os::Linux));
}

#[test]
fn jdtls_runs_with_the_users_java() {
    let launcher = "lib/jdtls/plugins/org.eclipse.equinox.launcher_1.8.0.v1.jar";
    let (dir, bundle) = bundle_with(&[launcher, "lib/jdtls/plugins/other.jar"]);
    let with_java = bundle
        .launch("jdtls", &context(Os::Linux, Some("/jdk/bin/java")))
        .unwrap();
    assert_eq!(with_java.program, Path::new("/jdk/bin/java"));
    assert_eq!(with_java.missing_runtime, None);
    let args = slashes(with_java.args.join(" "));
    let config = dir.path().join("lib/jdtls/config_linux");
    assert!(args.contains(&slashes(format!(
        "-Dosgi.sharedConfiguration.area={}",
        config.display()
    ))));
    assert!(args.contains(&slashes(format!(
        "-jar {}",
        dir.path().join(launcher).display()
    ))));
    assert!(args.contains("-configuration /state/jdtls/configuration"));
    assert!(
        args.ends_with("-data /state/jdtls/${workspaceFolderHash}"),
        "{args}"
    );
    let arm = LaunchContext {
        arm64: true,
        ..context(Os::MacOs, Some("/j"))
    };
    let mac = bundle.launch("jdtls", &arm).unwrap().args.join(" ");
    assert!(mac.contains("config_mac_arm"), "{mac}");
    // No Java: the launch says why it will fail.
    let without = bundle.launch("jdtls", &context(Os::Windows, None)).unwrap();
    assert_eq!(without.program, Path::new("java.exe"));
    assert_eq!(without.missing_runtime.as_deref(), Some(JAVA_MISSING));
    // An older Java starts, but the user is told why it fails.
    let old = LaunchContext {
        java_version: Some(17),
        ..context(Os::Linux, Some("/jdk17/bin/java"))
    };
    let launch = bundle.launch("jdtls", &old).unwrap();
    assert_eq!(launch.program, Path::new("/jdk17/bin/java"));
    let note = launch.missing_runtime.unwrap();
    assert!(note.contains("Java 21 or newer"), "{note}");
    assert!(note.contains("is Java 17"), "{note}");
    let recent = LaunchContext {
        java_version: Some(JDTLS_MIN_JAVA),
        ..context(Os::Linux, Some("/jdk/bin/java"))
    };
    assert_eq!(
        bundle.launch("jdtls", &recent).unwrap().missing_runtime,
        None
    );
}

#[test]
fn java_versions_come_from_the_release_file() {
    assert_eq!(parse_java_release("JAVA_VERSION=\"21.0.12.1\"\n"), Some(21));
    assert_eq!(
        parse_java_release("IMPLEMENTOR=\"x\"\nJAVA_VERSION=\"1.8.0_402\""),
        Some(8)
    );
    assert_eq!(parse_java_release("JAVA_VERSION=\"25-ea\""), Some(25));
    assert_eq!(parse_java_release("OS_NAME=\"Linux\""), None);

    let dir = tempfile::tempdir().unwrap();
    let jdk = dir.path().join("jdk-17");
    let java = jdk.join("bin").join("java");
    fs::create_dir_all(java.parent().unwrap()).unwrap();
    fs::write(&java, "").unwrap();
    assert_eq!(java_version(&java), None);
    fs::write(jdk.join("release"), "JAVA_VERSION=\"17.0.9\"\n").unwrap();
    assert_eq!(java_version(&java), Some(17));
    // A JRE inside a JDK: the file is one folder up.
    let jre_java = jdk.join("jre").join("bin").join("java");
    fs::create_dir_all(jre_java.parent().unwrap()).unwrap();
    fs::write(&jre_java, "").unwrap();
    assert_eq!(java_version(&jre_java), Some(17));
    // `/usr/bin/java`-style links are followed to the JDK.
    #[cfg(unix)]
    {
        let link = dir.path().join("java");
        std::os::unix::fs::symlink(&java, &link).unwrap();
        assert_eq!(java_version(&link), Some(17));
    }
}

#[test]
fn resolution_prefers_config_then_path_then_bundle() {
    let (dir, bundle) = bundle_with(&PYRIGHT_FILES);
    let cx = context(Os::Linux, None);
    let args = vec!["--stdio".to_string()];
    let launch = |command: &str| bundle.launch(command, &cx);

    // An explicit path is used as configured, even when bundled.
    let configured = resolve_server(
        "/opt/pyright/bin/pyright-langserver",
        &args,
        |_| true,
        launch,
    );
    assert_eq!(configured.source, ServerSource::Configured);
    assert_eq!(configured.command, "/opt/pyright/bin/pyright-langserver");

    // On PATH: the user's install wins over the bundle.
    let on_path = resolve_server("pyright-langserver", &args, |_| true, launch);
    assert_eq!(on_path.source, ServerSource::Path);
    assert_eq!(
        (on_path.command.as_str(), on_path.args.as_slice()),
        ("pyright-langserver", args.as_slice())
    );

    // Not on PATH: the bundled copy, with the configured arguments after
    // the script.
    let bundled = resolve_server("pyright-langserver", &args, |_| false, launch);
    assert_eq!(bundled.source, ServerSource::Bundled);
    assert_eq!(
        slashes(bundled.command.clone()),
        slashes(dir.path().join("node/bin/node").display().to_string())
    );
    assert_eq!(bundled.args.len(), 2);
    assert!(bundled.args[0].ends_with("langserver.index.js"));
    assert_eq!(bundled.args[1], "--stdio");

    // Neither: unchanged, reported as not installed when it starts.
    let missing = resolve_server("typos-lsp", &args, |_| false, launch);
    assert_eq!(missing.source, ServerSource::Missing);
    assert_eq!(missing.command, "typos-lsp");
}

#[test]
fn bundle_locations() {
    let dirs = bundle_dirs(
        Path::new("/opt/arcode/bin/arcode"),
        Some(Path::new("/home/me/.local/share/arcode")),
    );
    assert_eq!(
        dirs,
        [
            PathBuf::from("/opt/arcode/share/arcode/language-servers"),
            PathBuf::from("/home/me/.local/share/arcode/language-servers"),
        ]
    );
    // macOS app bundle: Contents/MacOS/arcode.
    assert_eq!(
        bundle_dirs(Path::new("/A.app/Contents/MacOS/arcode"), None),
        [PathBuf::from(
            "/A.app/Contents/share/arcode/language-servers"
        )]
    );
}

#[test]
fn loading_and_finding_bundles() {
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(Bundle::load(empty.path()), Ok(None));
    let future = tempfile::tempdir().unwrap();
    fs::write(
        future.path().join(MANIFEST_FILE),
        MINIMAL.replace("schema = 1", "schema = 2"),
    )
    .unwrap();
    assert!(
        Bundle::load(future.path())
            .unwrap_err()
            .contains("schema 2")
    );
    let (good_dir, good) = bundle_with(&[]);
    let dirs = [
        empty.path().to_path_buf(),
        future.path().to_path_buf(),
        good_dir.path().to_path_buf(),
    ];
    let (found, problems) = Bundle::find(&dirs);
    assert_eq!(found, Some(good));
    assert_eq!(problems.len(), 1);
}
