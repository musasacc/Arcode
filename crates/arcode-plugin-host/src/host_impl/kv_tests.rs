use super::*;

#[test]
fn keys_and_quota() {
    let mut s = KvScope::default();
    assert_eq!(s.set("", vec![]), Err(KvError::InvalidKey));
    assert_eq!(s.set("tab\t", vec![]), Err(KvError::InvalidKey));
    assert_eq!(s.set(&"k".repeat(129), vec![]), Err(KvError::InvalidKey));
    s.set("a/1", vec![0; 10]).unwrap();
    s.set("a/2", vec![0; 10]).unwrap();
    s.set("b", vec![]).unwrap();
    assert_eq!(s.keys("a/"), ["a/1", "a/2"]);
    assert_eq!(s.bytes(), 3 + 10 + 3 + 10 + 1);
    let big = KV_QUOTA_BYTES - s.bytes() - 1;
    assert_eq!(s.set("c", vec![0; big + 1]), Err(KvError::QuotaExceeded));
    s.set("c", vec![0; big]).unwrap();
    assert_eq!(s.bytes(), KV_QUOTA_BYTES);
    // Replacing a value frees the old one first.
    s.set("c", vec![0; big]).unwrap();
    assert!(s.delete("c"));
    assert!(!s.delete("missing"));
    assert_eq!(s.bytes(), 27);
    assert_eq!(s.get("a/1").map(<[u8]>::len), Some(10));
}

fn files(dir: &Path) -> KvFiles {
    KvFiles {
        global: Some(dir.join("plug/global.kv")),
        project: Some(dir.join("plug/0123.kv")),
    }
}

#[test]
fn scopes_are_separate_and_persist_per_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut kv = KvStore::new(files(dir.path()));
    kv.set(Scope::Global, "theme", b"dark".to_vec()).unwrap();
    kv.set(Scope::Project, "env", b"dev".to_vec()).unwrap();
    assert_eq!(kv.get(Scope::Project, "theme"), None);
    assert_eq!(kv.keys(Scope::Global, ""), ["theme"]);
    assert!(
        !dir.path().join("plug/global.kv").exists(),
        "written on flush"
    );
    kv.flush();
    let mut again = KvStore::new(files(dir.path()));
    assert_eq!(
        again.get(Scope::Global, "theme").as_deref(),
        Some(&b"dark"[..])
    );
    assert_eq!(
        again.get(Scope::Project, "env").as_deref(),
        Some(&b"dev"[..])
    );
    again.delete(Scope::Global, "theme");
    again.flush();
    let mut third = KvStore::new(files(dir.path()));
    assert!(third.keys(Scope::Global, "").is_empty());
    assert_eq!(third.keys(Scope::Project, ""), ["env"]);
}

#[test]
fn quota_holds_per_scope() {
    let mut kv = KvStore::new(KvFiles::none());
    let value = vec![7; KV_QUOTA_BYTES - 1];
    kv.set(Scope::Global, "g", value.clone()).unwrap();
    kv.set(Scope::Project, "p", value).unwrap();
    assert_eq!(
        kv.set(Scope::Global, "x", vec![1]),
        Err(KvError::QuotaExceeded)
    );
    assert_eq!(kv.bytes(Scope::Global), KV_QUOTA_BYTES);
    kv.flush(); // no files: nothing to do, nothing fails
}

#[test]
fn damaged_or_tampered_files_read_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let f = files(dir.path());
    let global = f.global.clone().unwrap();
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, b"ARCODE-KV 1\n\xff\xff").unwrap();
    let mut kv = KvStore::new(f.clone());
    assert!(kv.keys(Scope::Global, "").is_empty());
    // An over-quota file written by someone else is refused as a whole.
    let mut entries = BTreeMap::new();
    entries.insert("a".to_owned(), vec![0; KV_QUOTA_BYTES]);
    std::fs::write(&global, kv_file::encode(&entries)).unwrap();
    let mut kv = KvStore::new(f);
    assert_eq!(kv.get(Scope::Global, "a"), None);
    assert_eq!(kv.bytes(Scope::Global), 0);
}

#[test]
fn files_follow_the_documented_layout() {
    let paths = PluginPaths::with_bases(None, Path::new("/c"), Path::new("/d"), Path::new("/k"));
    let root = Path::new("/w/proj");
    let f = KvFiles::for_plugin(&paths, "a.b", Some(root));
    assert_eq!(
        f.global,
        Some(PathBuf::from("/d/arcode/plugin-data/a.b/global.kv"))
    );
    assert_eq!(f.project, Some(paths.kv_file("a.b", Some(root))));
    assert_eq!(KvFiles::for_plugin(&paths, "a.b", None).project, None);
}
