use std::fs;

use super::*;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    (dir, root)
}

fn kinds(found: &[MissingPackages]) -> Vec<PackageKind> {
    found.iter().map(|m| m.file.kind).collect()
}

#[test]
fn cargo_without_target_is_not_reported() {
    let (_d, root) = project();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"a\"\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
}

#[test]
fn package_json_without_node_modules() {
    let (_d, root) = project();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"left-pad":"1"}}"#,
    )
    .unwrap();
    let found = missing_packages_with(&root, &[], None);
    assert_eq!(kinds(&found), [PackageKind::Npm]);
    assert_eq!(found[0].display(), "npm install");
    fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
    assert_eq!(
        missing_packages_with(&root, &[], None)[0].display(),
        "pnpm install"
    );
    fs::create_dir(root.join("node_modules")).unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
}

#[test]
fn package_json_without_dependencies_needs_nothing() {
    let (_d, root) = project();
    fs::write(
        root.join("package.json"),
        r#"{"name":"x","scripts":{"start":"node ."}}"#,
    )
    .unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
}

#[test]
fn requirements_without_venv_create_one_first() {
    let (_d, root) = project();
    fs::write(root.join("requirements.txt"), "# deps\nrequests==2.31\n").unwrap();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"a\"\ndependencies = [\"rich\"]\n",
    )
    .unwrap();
    let found = missing_packages_with(&root, &[], None);
    assert_eq!(found.len(), 1, "one Python entry per folder: {found:?}");
    let steps = &found[0].steps;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].args, ["-m", "venv", ".venv"]);
    assert!(
        steps[1].display.starts_with(".venv/"),
        "{}",
        steps[1].display
    );
    assert!(
        steps[1]
            .args
            .starts_with(&["-m".to_string(), "pip".into(), "install".into()])
    );
}

#[test]
fn empty_requirements_and_existing_venv_are_fine() {
    let (_d, root) = project();
    fs::write(root.join("requirements.txt"), "# nothing yet\n\n").unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
    fs::write(root.join("requirements.txt"), "flask\n").unwrap();
    assert_eq!(missing_packages_with(&root, &[], None).len(), 1);
    let python = toolchain::venv_python(&root.join(".venv"));
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, "").unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
}

#[test]
fn uv_projects_use_uv_sync() {
    let (_d, root) = project();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"a\"\ndependencies = [\"rich\"]\n",
    )
    .unwrap();
    fs::write(root.join("uv.lock"), "").unwrap();
    let found = missing_packages_with(&root, &[], None);
    assert_eq!(found[0].display(), "uv sync");
}

#[test]
fn go_modules_are_looked_up_in_the_cache() {
    let (_d, root) = project();
    let cache = root.join("cache");
    fs::write(
        root.join("go.mod"),
        "module a\n\nrequire golang.org/x/text v0.14.0\n",
    )
    .unwrap();
    let found = missing_packages_with(&root, &[], Some(&cache));
    assert_eq!(kinds(&found), [PackageKind::Go]);
    assert_eq!(found[0].display(), "go mod download");
    assert!(
        missing_packages_with(&root, &[], None).is_empty(),
        "no cache: unknown"
    );
    fs::create_dir_all(cache.join("golang.org/x/text@v0.14.0")).unwrap();
    assert!(missing_packages_with(&root, &[], Some(&cache)).is_empty());
}

#[test]
fn gemfile_without_lock() {
    let (_d, root) = project();
    fs::write(
        root.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"rails\"\n",
    )
    .unwrap();
    let found = missing_packages_with(&root, &[], None);
    assert_eq!(found[0].display(), "bundle install");
}

#[test]
fn an_opened_file_brings_its_nested_manifest() {
    let (_d, root) = project();
    let web = root.join("web");
    fs::create_dir_all(web.join("src")).unwrap();
    fs::write(
        web.join("package.json"),
        r#"{"devDependencies":{"vite":"5"}}"#,
    )
    .unwrap();
    fs::write(web.join("src/main.ts"), "").unwrap();
    assert!(missing_packages_with(&root, &[], None).is_empty());
    let opened = [web.join("src/main.ts"), web.join("package.json")];
    let found = missing_packages_with(&root, &opened, None);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].file.install.cwd, web);
    let outside = [PathBuf::from("/elsewhere/package.json")];
    assert!(missing_packages_with(&root, &outside, None).is_empty());
}
