//! Every parser on truncated and mangled versions of every fixture: they
//! may return errors or partial results, but never panic.

use super::*;
use crate::deps::{
    bundler, cargo_deps, composer, go_deps, mini_yaml, node_deps, node_locks, python_deps,
    python_locks, requirements,
};

const FIXTURES: [&str; 30] = [
    fx!("cargo_root.toml"),
    fx!("cargo_member.toml"),
    fx!("Cargo.lock"),
    fx!("package.json"),
    fx!("package-lock.json"),
    fx!("yarn_classic.lock"),
    fx!("yarn_berry.lock"),
    fx!("pnpm_v9.yaml"),
    fx!("pnpm_v6.yaml"),
    fx!("bun.lock"),
    fx!("pyproject_uv.toml"),
    fx!("uv.lock"),
    fx!("pyproject_poetry.toml"),
    fx!("poetry.lock"),
    fx!("Pipfile"),
    fx!("Pipfile.lock"),
    fx!("requirements.txt"),
    fx!("composer.json"),
    fx!("composer.lock"),
    fx!("Gemfile"),
    fx!("Gemfile.lock"),
    fx!("go.mod"),
    fx!("npm_outdated.json"),
    fx!("yarn_outdated.jsonl"),
    fx!("pip_outdated.json"),
    fx!("go_outdated.json"),
    fx!("composer_outdated.json"),
    fx!("cargo_dry_run.txt"),
    fx!("poetry_outdated.txt"),
    fx!("bundle_outdated.txt"),
];

fn every_parser(text: &str) {
    let _ = cargo_deps::parse(text, &Default::default());
    let _ = cargo_deps::lock_versions(text);
    let _ = cargo_deps::workspace_requirements(text);
    let _ = cargo_deps::pick_version(text, &[text.to_string(), "1.2.3".into()]);
    let _ = node_deps::parse(text);
    for name in [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lock",
        "x",
    ] {
        let _ = node_locks::parse(name, text);
    }
    let _ = mini_yaml::parse(text);
    let _ = python_deps::parse_pyproject(text);
    let _ = python_deps::parse_pipfile(text);
    let _ = requirements::parse_requirements_file(text);
    let _ = requirements::parse_requirement(text);
    let _ = python_locks::toml_lock(text);
    let _ = python_locks::pipfile_lock(text);
    let _ = go_deps::parse(text);
    let _ = composer::parse(text);
    let _ = composer::lock_versions(text);
    let _ = bundler::parse(text);
    let _ = bundler::lock_versions(text);
    use OutdatedFormat as F;
    for format in [
        F::NpmJson,
        F::YarnJson,
        F::PipJson,
        F::GoJson,
        F::ComposerJson,
        F::CargoDryRun,
        F::UvDryRun,
        F::PoetryText,
        F::BundlerParseable,
        F::Raw,
    ] {
        let _ = parse_outdated(format, text, text, false);
        let _ = parse_outdated(format, text, text, true);
    }
}

#[test]
fn truncated_fixtures_never_panic() {
    for fixture in FIXTURES {
        let cuts: Vec<usize> = fixture
            .char_indices()
            .map(|(i, _)| i)
            .filter(|i| i % 5 == 0)
            .chain([fixture.len()])
            .collect();
        for cut in cuts {
            every_parser(&fixture[..cut]);
            every_parser(&fixture[cut..]);
        }
    }
}

#[test]
fn mangled_input_never_panics() {
    let odd = [
        "",
        "\u{0}",
        "[",
        "]]",
        "{",
        "}",
        "\"",
        "'",
        "\\",
        ":",
        "- ",
        "@",
        "@@",
        "é@ü",
        "=",
        "a =",
        "[a]\nb = [",
        "gem",
        "gem(",
        "gem \"",
        "require (",
        "replace => x",
        "    x (",
        "-e",
        "-e #egg=",
        "#egg=",
        "x @",
        "{\"packages\": {\"node_modules/\": {\"version\": 1}}}",
        "{\"packages\": {\"\": [\"@\"]}}",
        "importers:\n  .:\n    dependencies:\n      a: {",
        "\"a\", \"b\":\n  version",
        "  version \"",
    ];
    for text in odd {
        every_parser(text);
        every_parser(&text.repeat(3));
    }
    for fixture in FIXTURES {
        every_parser(&fixture.replace('\n', "\r\n"));
        every_parser(&fixture.replace('"', "'"));
        every_parser(&fixture.replace(['{', '['], ""));
    }
}

#[test]
fn unreadable_files_are_reported() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("Cargo.toml")).unwrap();
    write(
        &root,
        "sub/package.json",
        "\u{feff}{\"name\": 1, \"dependencies\": [1, 2]}",
    );
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["npm sub/package.json"],
        "a folder named like a manifest is skipped"
    );
    assert!(found[0].deps.is_empty());
}

/// A repository can hold symlinks to devices: reading one would never
/// end. Manifests, lockfiles and the ancestors looked up for workspace
/// settings are read only when they are regular files.
#[cfg(unix)]
#[test]
fn special_files_are_not_read() {
    let (_dir, root) = project();
    std::os::unix::fs::symlink("/dev/zero", root.join("Cargo.toml")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", root.join("package.json")).unwrap();
    write(
        &root,
        "crates/a/Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    std::os::unix::fs::symlink("/dev/zero", root.join("crates/a/Cargo.lock")).unwrap();
    write(
        &root,
        "web/package.json",
        "{\"dependencies\": {\"left-pad\": \"1\"}}",
    );
    std::os::unix::fs::symlink("/dev/zero", root.join("web/yarn.lock")).unwrap();
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["cargo crates/a/Cargo.toml", "npm web/package.json"]
    );
    assert_eq!(dep(&found[0], "serde").requested, "1");
    assert!(found[0].lockfile.is_none());
    assert_eq!(dep(&found[1], "left-pad").requested, "1");
}
