//! Tests of the Packages model: manifests and lockfiles read from
//! realistic fixtures (`fixtures/`), commands, check output parsers and
//! malformed input.

mod commands;
mod malformed;
mod node;
mod outdated;
mod python;

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// A fixture's text.
macro_rules! fx {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name))
    };
}
pub(crate) use fx;

/// A temporary project folder (canonical path).
pub(super) fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    (dir, root)
}

/// Writes `text` to `root/rel`, creating folders.
pub(super) fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// The dependency `name` of `m`.
pub(super) fn dep<'a>(m: &'a ProjectManifest, name: &str) -> &'a Dependency {
    m.deps
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no dependency {name} in {:?}", m.path))
}

/// Manifests relative to `root`, `/`-separated, with their manager.
pub(super) fn listing(found: &[ProjectManifest], root: &Path) -> Vec<String> {
    found
        .iter()
        .map(|m| {
            let rel = m.path.strip_prefix(root).unwrap_or(&m.path);
            let rel: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            format!("{} {}", m.manager.label(), rel.join("/"))
        })
        .collect()
}

#[test]
fn cargo_workspace_with_inherited_and_locked_versions() {
    let (_dir, root) = project();
    write(&root, "Cargo.toml", fx!("cargo_root.toml"));
    write(&root, "Cargo.lock", fx!("Cargo.lock"));
    write(&root, "crates/core/Cargo.toml", fx!("cargo_member.toml"));
    write(
        &root,
        "crates/core/target/x/Cargo.toml",
        "[package]\nname = \"ignored\"\n",
    );
    write(
        &root,
        "target/debug/Cargo.toml",
        "[package]\nname = \"ignored\"\n",
    );
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["cargo Cargo.toml", "cargo crates/core/Cargo.toml"]
    );

    let ws = &found[0];
    assert!(ws.workspace_root);
    assert_eq!(
        ws.lockfile.as_deref(),
        Some(root.join("Cargo.lock").as_path())
    );
    assert_eq!(dep(ws, "serde").group.as_deref(), Some("workspace"));
    assert_eq!(dep(ws, "serde").line, Some(5));

    let m = &found[1];
    assert_eq!(m.name.as_deref(), Some("app-core"));
    assert_eq!(m.dir, root.join("crates/core"));
    let serde = dep(m, "serde");
    assert_eq!(serde.source, DepSource::Workspace);
    assert_eq!(serde.requested, "1.0.190", "inherited from the workspace");
    assert_eq!(serde.locked.as_deref(), Some("1.0.195"));
    assert_eq!(serde.line, Some(6));
    let anyhow = dep(m, "anyhow");
    assert_eq!((anyhow.kind, anyhow.line), (DepKind::Normal, Some(7)));
    assert_eq!(anyhow.locked.as_deref(), Some("1.0.79"));
    let rand = dep(m, "rand08");
    assert_eq!(rand.package, "rand");
    assert_eq!(
        rand.locked.as_deref(),
        Some("0.8.5"),
        "the version matching ^0.8"
    );
    assert_eq!(dep(m, "tokio").kind, DepKind::Optional);
    assert_eq!(
        dep(m, "local-util").source,
        DepSource::Path("../util".into())
    );
    assert!(matches!(dep(m, "fancy").source, DepSource::Git(_)));
    assert_eq!(dep(m, "regex").line, Some(13));
    assert_eq!(dep(m, "regex").requested, "1.10");
    let tempfile = dep(m, "tempfile");
    assert_eq!((tempfile.kind, tempfile.line), (DepKind::Dev, Some(18)));
    assert_eq!(tempfile.locked.as_deref(), Some("3.9.0"));
    assert_eq!(dep(m, "cc").kind, DepKind::Build);
    let winapi = dep(m, "winapi");
    assert_eq!(winapi.group.as_deref(), Some("cfg(windows)"));
    assert_eq!(winapi.line, Some(24));
}

#[test]
fn go_composer_and_bundler() {
    let (_dir, root) = project();
    write(&root, "server/go.mod", fx!("go.mod"));
    write(&root, "shop/composer.json", fx!("composer.json"));
    write(&root, "shop/composer.lock", fx!("composer.lock"));
    write(&root, "shop/vendor/x/composer.json", "{}");
    write(&root, "site/Gemfile", fx!("Gemfile"));
    write(&root, "site/Gemfile.lock", fx!("Gemfile.lock"));
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        [
            "go server/go.mod",
            "composer shop/composer.json",
            "bundler site/Gemfile"
        ]
    );

    let go = &found[0];
    assert_eq!(go.name.as_deref(), Some("github.com/example/server"));
    let gin = dep(go, "github.com/gin-gonic/gin");
    assert_eq!(
        (gin.kind, gin.line, gin.locked.as_deref()),
        (DepKind::Normal, Some(5), Some("v1.9.1"))
    );
    assert_eq!(dep(go, "gopkg.in/yaml.v3").kind, DepKind::Indirect);

    let php = &found[1];
    let names: Vec<_> = php.deps.iter().map(|d| d.name.as_str()).collect();
    assert!(!names.contains(&"php") && !names.contains(&"ext-json"));
    let laravel = dep(php, "laravel/framework");
    assert_eq!(
        (laravel.locked.as_deref(), laravel.line),
        (Some("v10.41.0"), Some(6))
    );
    assert_eq!(dep(php, "phpunit/phpunit").kind, DepKind::Dev);

    let ruby = &found[2];
    let rails = dep(ruby, "rails");
    assert_eq!(
        (
            rails.requested.as_str(),
            rails.locked.as_deref(),
            rails.line
        ),
        ("~> 7.1.2", Some("7.1.2"), Some(4))
    );
    assert_eq!(dep(ruby, "pg").requested, ">= 0.18, < 2.0");
    let debug = dep(ruby, "debug");
    assert_eq!(
        (debug.kind, debug.group.as_deref()),
        (DepKind::Dev, Some("development, test"))
    );
    let lograge = dep(ruby, "lograge");
    assert_eq!(
        (lograge.kind, lograge.group.as_deref()),
        (DepKind::Normal, Some("production"))
    );
    assert_eq!(dep(ruby, "capybara").kind, DepKind::Dev);
    assert_eq!(
        dep(ruby, "internal_gem").source,
        DepSource::Path("../internal_gem".into())
    );
    assert_eq!(dep(ruby, "fancy").locked.as_deref(), Some("0.3.0"));
    assert!(matches!(dep(ruby, "fancy").source, DepSource::Git(_)));
}

#[test]
fn package_files_and_skipped_folders() {
    assert!(is_package_file(Path::new("/p/Cargo.lock")));
    assert!(is_package_file(Path::new("/p/requirements-dev.txt")));
    assert!(is_package_file(Path::new("/p/requirements/base.txt")));
    assert!(is_package_file(Path::new("/p/pnpm-lock.yaml")));
    assert!(!is_package_file(Path::new("/p/notes.txt")));
    assert!(!is_package_file(Path::new("/p/src/main.rs")));

    let (_dir, root) = project();
    write(&root, "node_modules/x/package.json", "{}");
    write(&root, ".hidden/package.json", "{}");
    write(&root, ".venv/lib/pyproject.toml", "[project]\nname='x'\n");
    write(
        &root,
        "tools/pyproject.toml",
        "[tool.black]\nline-length = 100\n",
    );
    write(&root, "a/b/c/d/e/f/g/h/package.json", "{}");
    assert!(
        scan_manifests(&root).is_empty(),
        "{:?}",
        listing(&scan_manifests(&root), &root)
    );
}

#[test]
fn malformed_manifests_and_lockfiles_report_errors() {
    let (_dir, root) = project();
    write(&root, "bad-cargo/Cargo.toml", "[package\nname = ");
    write(&root, "bad-npm/package.json", "{ \"dependencies\": ");
    write(
        &root,
        "bad-lock/package.json",
        "{\"dependencies\": {\"a\": \"1\"}}",
    );
    write(&root, "bad-lock/package-lock.json", "{ nope");
    let found = scan_manifests(&root);
    assert_eq!(found.len(), 3);
    assert!(found[0].error.is_some() && found[0].deps.is_empty());
    let lock = found
        .iter()
        .find(|m| m.path.ends_with("bad-lock/package.json"))
        .unwrap();
    assert_eq!(lock.deps.len(), 1, "the manifest is still listed");
    assert!(
        lock.error
            .as_deref()
            .unwrap_or("")
            .contains("package-lock.json")
    );
    assert!(
        found
            .iter()
            .any(|m| m.path.ends_with("bad-npm/package.json") && m.error.is_some())
    );
}

#[test]
fn docs_urls() {
    assert_eq!(
        Manager::Cargo.docs_url("serde").as_deref(),
        Some("https://docs.rs/serde")
    );
    assert_eq!(
        Manager::Pnpm.docs_url("@types/node").as_deref(),
        Some("https://www.npmjs.com/package/@types/node")
    );
    assert_eq!(
        Manager::Uv.docs_url("httpx").as_deref(),
        Some("https://pypi.org/project/httpx/")
    );
    assert_eq!(
        Manager::Go.docs_url("github.com/gin-gonic/gin").as_deref(),
        Some("https://pkg.go.dev/github.com/gin-gonic/gin")
    );
    assert_eq!(
        Manager::Composer.docs_url("laravel/framework").as_deref(),
        Some("https://packagist.org/packages/laravel/framework")
    );
    assert_eq!(
        Manager::Bundler.docs_url("rails").as_deref(),
        Some("https://rubygems.org/gems/rails")
    );
    assert_eq!(Manager::Npm.docs_url("a?b=c"), None);
    assert_eq!(normalize_python("Flask_Login..x"), "flask-login-x");
}
