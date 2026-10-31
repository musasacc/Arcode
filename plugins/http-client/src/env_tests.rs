//! `env` tests (JetBrains-style environment files).

use super::*;
use crate::test_support::TempDir;
use arcode_plugin::testing;

const PUBLIC: &str = r#"{
  "$shared": { "version": "v1", "host": "shared.example" },
  "development": { "host": "localhost", "port": 8080, "secure": false, "nothing": null },
  "production": { "host": "example.com", "Security": { "Auth": {} } }
}"#;

const PRIVATE: &str = r#"{
  "development": { "token": "dev-secret" },
  "production": { "token": "prod-secret", "host": "private.example.com" },
  "local-only": { "x": "1" }
}"#;

#[test]
fn public_and_private_files_merge_with_shared_values() {
    let envs = Environments::parse(Some(PUBLIC), Some(PRIVATE)).unwrap();
    assert_eq!(envs.names(), ["development", "local-only", "production"]);
    let dev = envs.variables(Some("development"));
    assert_eq!(dev["host"], "localhost", "environment over $shared");
    assert_eq!(dev["version"], "v1");
    assert_eq!(dev["port"], "8080");
    assert_eq!(dev["secure"], "false");
    assert_eq!(dev["nothing"], "");
    assert_eq!(dev["token"], "dev-secret");
    let prod = envs.variables(Some("production"));
    assert_eq!(prod["host"], "private.example.com", "private over public");
    assert!(!prod.contains_key("Security"), "objects are skipped");
    let none = envs.variables(None);
    assert_eq!(none["host"], "shared.example");
    assert_eq!(envs.variables(Some("missing")), none);
    assert_eq!(envs.variables(Some("$shared")), none);
    assert!(envs.contains("production") && !envs.contains("$shared"));
}

#[test]
fn invalid_files_are_errors_and_missing_ones_are_empty() {
    assert_eq!(Environments::parse(None, None), Ok(Environments::default()));
    let err = Environments::parse(Some("{"), None).unwrap_err();
    assert!(
        err.starts_with("http-client.env.json: invalid JSON"),
        "{err}"
    );
    let err = Environments::parse(None, Some("[]")).unwrap_err();
    assert!(err.contains("private"), "{err}");
    let err = Environments::parse(Some(r#"{"dev": 1}"#), None).unwrap_err();
    assert!(err.contains("`dev`"), "{err}");
}

#[test]
fn search_dirs_walk_up_to_the_project() {
    let p = Path::new("/project");
    assert_eq!(
        search_dirs(p, Some("api/v1/users.http")),
        [
            PathBuf::from("/project/api/v1"),
            PathBuf::from("/project/api"),
            PathBuf::from("/project"),
        ]
    );
    assert_eq!(search_dirs(p, Some("a.http")), [PathBuf::from("/project")]);
    assert_eq!(search_dirs(p, None), [PathBuf::from("/project")]);
    assert_eq!(
        search_dirs(p, Some("../x/a.http")),
        [PathBuf::from("/project")]
    );
    assert_eq!(
        search_dirs(p, Some("/etc/a.http")),
        [PathBuf::from("/project")]
    );
}

#[test]
fn load_finds_the_nearest_files() {
    let dir = TempDir::new("env-load");
    let root = dir.path();
    std::fs::create_dir_all(root.join("api/v1")).unwrap();
    std::fs::write(root.join(PUBLIC_FILE), r#"{"dev": {"host": "root"}}"#).unwrap();
    std::fs::write(
        root.join("api").join(PUBLIC_FILE),
        r#"{"dev": {"host": "api"}}"#,
    )
    .unwrap();
    std::fs::write(root.join(PRIVATE_FILE), r#"{"dev": {"token": "t"}}"#).unwrap();
    let envs = Environments::load(root, Some("api/v1/users.http")).unwrap();
    let dev = envs.variables(Some("dev"));
    assert_eq!(dev["host"], "api", "nearest public file");
    assert_eq!(dev["token"], "t", "private file from the root");
    let envs = Environments::load(root, Some("other.http")).unwrap();
    assert_eq!(envs.variables(Some("dev"))["host"], "root");
    // No preopen (workspace denied) reads as no environments.
    let missing = Environments::load(Path::new("/nonexistent-project"), Some("a.http"));
    assert_eq!(missing, Ok(Environments::default()));
}

#[test]
fn selection_lives_in_project_kv() {
    testing::reset();
    assert_eq!(selected(), None);
    select(Some("dev")).unwrap();
    assert_eq!(selected().as_deref(), Some("dev"));
    assert!(testing::with(|h| h
        .kv
        .contains_key(&(kv::Scope::Project, "environment".into()))));
    select(None).unwrap();
    assert_eq!(selected(), None);
}
