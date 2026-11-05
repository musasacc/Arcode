use serde_json::json;

use super::*;

fn ra() -> ServerConfig {
    ServerConfig::new("rust-analyzer", "rust-analyzer", &["Cargo.toml"])
}

#[test]
fn recognises_rust_analyzer_by_name_command_or_argument() {
    assert!(is_rust_analyzer(&ra()));
    let mut c = ServerConfig::new("ra", "/opt/bin/rust-analyzer", &[]);
    assert!(is_rust_analyzer(&c));
    c.command = "rustup".into();
    c.args = vec!["run".into(), "stable".into(), "rust-analyzer".into()];
    assert!(is_rust_analyzer(&c));
    assert!(!is_rust_analyzer(&ServerConfig::new("gopls", "gopls", &[])));
}

#[test]
fn loose_files_have_no_project_above_them() {
    let tmp = tempfile::tempdir().unwrap();
    // The test's own temporary folder could sit inside a project.
    if !is_loose(&ra(), &tmp.path().join("probe.rs")) {
        eprintln!("skipped: the temporary folder is inside a Rust project");
        return;
    }
    let scratch = tmp.path().join("scratch");
    let cargo = tmp.path().join("proj");
    let json = tmp.path().join("json");
    for dir in [&scratch, &cargo.join("src/bin"), &json] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(cargo.join("Cargo.toml"), "").unwrap();
    std::fs::write(json.join("rust-project.json"), "{}").unwrap();
    assert!(is_loose(&ra(), &scratch.join("test.rs")));
    assert!(!is_loose(&ra(), &cargo.join("src/bin/tool.rs")));
    assert!(!is_loose(&ra(), &cargo.join("build.rs")));
    assert!(!is_loose(&ra(), &json.join("main.rs")));
    // Only rust-analyzer serves files this way.
    let gopls = ServerConfig::new("gopls", "gopls", &["go.mod"]);
    assert!(!is_loose(&gopls, &scratch.join("main.go")));
}

#[test]
fn options_name_the_files_and_turn_cargo_check_off() {
    let files = [PathBuf::from("/s/a.rs"), PathBuf::from("/s/b.rs")];
    assert_eq!(
        with_detached_files(None, &files),
        json!({ "detachedFiles": ["/s/a.rs", "/s/b.rs"], "checkOnSave": false })
    );
    // Configured settings are kept, an explicit `checkOnSave` too.
    let own = json!({ "checkOnSave": true, "cargo": { "features": "all" } });
    assert_eq!(
        with_detached_files(Some(own), &files[..1]),
        json!({
            "detachedFiles": ["/s/a.rs"],
            "checkOnSave": true,
            "cargo": { "features": "all" },
        })
    );
}

const SMALL: FolderLimit = FolderLimit {
    rust_files: 3,
    entries: 50,
};

/// A folder with `names` as Rust files, which exist.
fn folder(root: &Path, dir: &str, names: &[&str]) -> Vec<PathBuf> {
    let dir = root.join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    names
        .iter()
        .map(|n| {
            std::fs::write(dir.join(n), "fn main() {}\n").unwrap();
            dir.join(n)
        })
        .collect()
}

fn options(files: &[&PathBuf]) -> Value {
    json!({ "cargo": {}, "detachedFiles": files, "checkOnSave": false })
}

fn send(files: &[&PathBuf]) -> Outcome<Update> {
    Outcome {
        action: Update::Send(options(files)),
        refused: Vec::new(),
    }
}

fn unchanged() -> Outcome<Update> {
    Outcome {
        action: Update::Unchanged,
        refused: Vec::new(),
    }
}

#[test]
fn files_added_after_the_start_are_sent() {
    let tmp = tempfile::tempdir().unwrap();
    let [a, b] = folder(tmp.path(), "s", &["a.rs", "b.rs"])
        .try_into()
        .unwrap();
    let base = Some(json!({ "cargo": {} }));
    let files = DetachedFiles::new(base.clone(), SMALL, Some(&a));
    // Before the start the launch takes everything.
    assert_eq!(files.update(), unchanged());
    assert_eq!(files.add(&a), Added::Known);
    let launch = files.launch();
    assert_eq!(launch.action, Launch::Detached(options(&[&a])));
    assert_eq!(files.add(&b), Added::New);
    assert_eq!(files.update(), send(&[&a, &b]));
    assert_eq!(files.update(), unchanged());
    // An instance without loose files keeps its configured options.
    let other = DetachedFiles::new(base, SMALL, None);
    assert_eq!(other.launch().action, Launch::Configured);
    assert_eq!(other.update(), unchanged());
}

#[test]
fn a_missing_file_in_the_list_is_noticed() {
    let tmp = tempfile::tempdir().unwrap();
    let [a, b] = folder(tmp.path(), "s", &["a.rs", "b.rs"])
        .try_into()
        .unwrap();
    let files = DetachedFiles::new(Some(json!({ "cargo": {} })), SMALL, Some(&a));
    // Nothing is sent before the start.
    std::fs::remove_file(&a).unwrap();
    assert!(!files.names_missing());
    std::fs::write(&a, "").unwrap();
    files.add(&b);
    files.launch();
    assert!(!files.names_missing());
    // Closing alone changes nothing: b.rs still exists.
    files.remove(&b);
    assert_eq!(files.update(), unchanged());
    assert!(!files.names_missing());
    // a.rs renamed: the manager restarts the instance.
    let c = a.with_file_name("c.rs");
    std::fs::rename(&a, &c).unwrap();
    assert!(files.names_missing());
    // Should a new file get here first, the missing one is dropped too.
    files.remove(&a);
    files.add(&c);
    assert_eq!(files.update(), send(&[&c]));
    assert!(!files.names_missing());
    // With no file left there is nothing to send.
    std::fs::remove_file(&c).unwrap();
    assert_eq!(files.update(), unchanged());
    assert!(files.names_missing());
}

#[test]
fn a_file_gone_before_the_start_starts_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.rs");
    let files = DetachedFiles::new(None, SMALL, Some(&a));
    assert_eq!(files.launch().action, Launch::Nothing);
    // Retired: a file opened now needs a new instance.
    assert!(files.is_retired());
    assert_eq!(files.add(&a), Added::Retired);
}

#[test]
fn folders_too_big_to_analyse_are_refused_once() {
    let tmp = tempfile::tempdir().unwrap();
    let big = folder(tmp.path(), "big", &["a.rs", "b.rs", "c.rs", "d.rs"]);
    let files = DetachedFiles::new(None, SMALL, Some(&big[0]));
    let dir = tmp.path().join("big");
    assert_eq!(
        files.launch(),
        Outcome {
            action: Launch::Nothing,
            refused: vec![dir.clone()],
        }
    );
    // A file of the big folder joining a served one is not analysed; the
    // folder is reported once.
    let [small] = folder(tmp.path(), "small", &["x.rs"]).try_into().unwrap();
    let files = DetachedFiles::new(None, SMALL, Some(&small));
    files.launch();
    files.add(&big[1]);
    assert_eq!(
        files.update(),
        Outcome {
            action: Update::Unchanged,
            refused: vec![dir],
        }
    );
    files.add(&big[2]);
    assert_eq!(files.update(), unchanged());
    assert!(refused_message(Path::new("/home/me")).contains("/home/me"));
}
