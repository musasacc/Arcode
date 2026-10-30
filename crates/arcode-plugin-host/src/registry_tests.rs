//! Registry and discovery tests against temporary plugin folders.

use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::{
    ActivationEvent, ManifestError, PluginId, PluginSource, QualifiedId, UserLimits,
};
use crate::registry::{DiscoveryRoots, Registry, load};

fn manifest(id: &str, name: &str, extra: &str) -> String {
    format!(
        "[plugin]\nid = \"{id}\"\nname = \"{name}\"\nversion = \"0.1.0\"\napi = \"0.1\"\n{extra}"
    )
}

fn write_plugin(root: &Path, folder: &str, text: &str) -> PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plugin.toml"), text).unwrap();
    dir
}

fn id(s: &str) -> PluginId {
    PluginId::parse(s).unwrap()
}

#[test]
fn discovery_order_is_bundled_then_installed_then_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let installed = tmp.path().join("installed");
    write_plugin(&bundled, "acme.tool", &manifest("acme.tool", "Bundled", ""));
    write_plugin(
        &bundled,
        "acme.only-bundled",
        &manifest("acme.only-bundled", "B", ""),
    );
    write_plugin(
        &installed,
        "acme.tool",
        &manifest("acme.tool", "Installed", ""),
    );
    let dev = write_plugin(
        &tmp.path().join("anywhere"),
        "my-dev-folder",
        &manifest("acme.tool", "Dev", ""),
    );

    let roots = DiscoveryRoots {
        bundled: vec![bundled.clone()],
        installed: vec![installed.clone()],
        dev: vec![],
    };
    let reg = Registry::discover(&roots, &UserLimits::default());
    let tool = reg.get(&id("acme.tool")).unwrap();
    assert_eq!(
        (tool.manifest.plugin.name.as_str(), tool.source),
        ("Installed", PluginSource::Installed)
    );
    assert_eq!(
        tool.shadowed,
        vec![(PluginSource::Bundled, bundled.join("acme.tool"))]
    );

    let roots = DiscoveryRoots {
        dev: vec![dev.clone()],
        ..roots
    };
    let reg = Registry::discover(&roots, &UserLimits::default());
    let tool = reg.get(&id("acme.tool")).unwrap();
    assert_eq!(
        (tool.manifest.plugin.name.as_str(), tool.source, &tool.dir),
        ("Dev", PluginSource::Dev, &dev)
    );
    assert_eq!(tool.shadowed.len(), 2);
    assert!(reg.get(&id("acme.only-bundled")).is_some());
    assert_eq!(reg.plugins().count(), 2);
    assert!(reg.problems().is_empty(), "{:?}", reg.problems());
}

#[test]
fn an_installed_plugin_cannot_take_over_a_reserved_bundled_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (bundled, installed) = (tmp.path().join("b"), tmp.path().join("i"));
    write_plugin(
        &bundled,
        "arcode.http-client",
        &manifest("arcode.http-client", "Real", ""),
    );
    write_plugin(
        &installed,
        "arcode.http-client",
        &manifest("arcode.http-client", "Fake", ""),
    );
    let roots = DiscoveryRoots {
        bundled: vec![bundled],
        installed: vec![installed],
        dev: vec![],
    };
    let reg = Registry::discover(&roots, &UserLimits::default());
    assert_eq!(
        reg.get(&id("arcode.http-client"))
            .unwrap()
            .manifest
            .plugin
            .name,
        "Real"
    );
    assert!(matches!(reg.problems(), [p] if matches!(p.error, ManifestError::ReservedId(_))));
}

#[test]
fn bad_folders_become_problems_not_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("installed");
    write_plugin(&root, "acme.good", &manifest("acme.good", "Good", ""));
    write_plugin(&root, "wrong-name", &manifest("acme.other", "Other", ""));
    write_plugin(&root, "acme.broken", "[plugin\n");
    write_plugin(
        &root,
        "acme.unknown",
        &manifest("acme.unknown", "U", "startup = true"),
    );
    fs::create_dir_all(root.join("acme.empty")).unwrap();
    fs::write(root.join("stray-file.toml"), "x").unwrap();
    let roots = DiscoveryRoots {
        installed: vec![root, tmp.path().join("does-not-exist")],
        ..DiscoveryRoots::default()
    };
    let reg = Registry::discover(&roots, &UserLimits::default());
    assert_eq!(
        reg.plugins().map(|p| p.id().as_str()).collect::<Vec<_>>(),
        ["acme.good"]
    );
    assert_eq!(reg.problems().len(), 4, "{:#?}", reg.problems());
    assert!(
        reg.problems()
            .iter()
            .any(|p| p.error.to_string().contains("does not match plugin id"))
    );
}

#[test]
fn oversized_manifest_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let padding = "#".repeat(300 * 1024);
    let dir = write_plugin(
        tmp.path(),
        "acme.big",
        &format!("{padding}\n{}", manifest("acme.big", "Big", "")),
    );
    assert!(
        matches!(load(&dir, PluginSource::Installed), Err(ManifestError::Io(m)) if m.contains("larger"))
    );
}

#[cfg(unix)]
#[test]
fn symlinked_manifest_is_refused_outside_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let real = write_plugin(tmp.path(), "real", &manifest("acme.link", "Link", ""));
    let dir = tmp.path().join("installed").join("acme.link");
    fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(real.join("plugin.toml"), dir.join("plugin.toml")).unwrap();
    assert!(matches!(
        load(&dir, PluginSource::Installed),
        Err(ManifestError::Io(_))
    ));
    assert!(load(&dir, PluginSource::Dev).is_ok());
}

const HTTP: &str = include_str!("manifest/testdata/http-client.toml");
const DB: &str = include_str!("manifest/testdata/db-viewer.toml");

fn reference_registry(user: &UserLimits) -> (tempfile::TempDir, Registry) {
    let tmp = tempfile::tempdir().unwrap();
    write_plugin(tmp.path(), "arcode.http-client", HTTP);
    write_plugin(tmp.path(), "arcode.db-viewer", DB);
    let roots = DiscoveryRoots {
        bundled: vec![tmp.path().to_owned()],
        ..DiscoveryRoots::default()
    };
    let reg = Registry::discover(&roots, user);
    (tmp, reg)
}

#[test]
fn reference_plugins_register_their_contributions() {
    let (_tmp, reg) = reference_registry(&UserLimits::default());
    assert!(reg.problems().is_empty(), "{:?}", reg.problems());
    let commands: Vec<_> = reg.commands().map(|(q, _, _)| q.to_string()).collect();
    assert_eq!(
        commands,
        [
            "arcode.db-viewer/refresh",
            "arcode.db-viewer/new-query",
            "arcode.http-client/send",
            "arcode.http-client/select-environment",
        ]
    );
    let send = QualifiedId::parse("arcode.http-client/send").unwrap();
    let (plugin, cmd) = reg.command(&send).unwrap();
    assert_eq!(
        (plugin.id().as_str(), cmd.title.as_str()),
        ("arcode.http-client", "HTTP: Send Request at Cursor")
    );
    assert!(
        reg.command(&QualifiedId::parse("arcode.http-client/nope").unwrap())
            .is_none()
    );
    let panel = QualifiedId::parse("arcode.db-viewer/databases").unwrap();
    assert_eq!(reg.panel(&panel).unwrap().1.title, "Databases");
    assert_eq!(
        reg.languages()
            .map(|(_, l)| l.id.as_str())
            .collect::<Vec<_>>(),
        ["http"]
    );
    assert_eq!(reg.lens_providers("http").len(), 1);
    assert!(reg.lens_providers("rust").is_empty());
}

#[test]
fn openers_match_file_names_and_paths() {
    let (_tmp, reg) = reference_registry(&UserLimits::default());
    let sqlite = Some("arcode.db-viewer/sqlite");
    assert_eq!(
        reg.opener_for(Path::new("data/app.sqlite3"))
            .map(QualifiedId::as_str),
        sqlite
    );
    assert_eq!(
        reg.opener_for(Path::new("x.db")).map(QualifiedId::as_str),
        sqlite
    );
    assert_eq!(reg.opener_for(Path::new("x.db.bak")), None);
    assert_eq!(reg.opener_for(Path::new("db")), None);

    let tmp = tempfile::tempdir().unwrap();
    let extra = "[[contributes.openers]]\nid = \"fixtures\"\ntitle = \"F\"\npatterns = [\"fixtures/*.bin\"]";
    let dir = write_plugin(tmp.path(), "dev", &manifest("acme.bin", "Bin", extra));
    let reg = Registry::discover(
        &DiscoveryRoots {
            dev: vec![dir],
            ..DiscoveryRoots::default()
        },
        &UserLimits::default(),
    );
    assert_eq!(
        reg.opener_for(Path::new("fixtures/a.bin"))
            .map(QualifiedId::as_str),
        Some("acme.bin/fixtures")
    );
    assert_eq!(
        reg.opener_for(Path::new("fixtures/sub/a.bin")),
        None,
        "`*` must not cross directories"
    );
    assert_eq!(reg.opener_for(Path::new("a.bin")), None);
}

#[test]
fn activation_events_route_to_plugins() {
    let (_tmp, reg) = reference_registry(&UserLimits::default());
    let opener = ActivationEvent::Opener(QualifiedId::parse("arcode.db-viewer/sqlite").unwrap());
    assert_eq!(
        reg.plugins_for(&opener)
            .iter()
            .map(|p| p.id().as_str())
            .collect::<Vec<_>>(),
        ["arcode.db-viewer"]
    );
    let other = ActivationEvent::Panel(QualifiedId::parse("arcode.db-viewer/nope").unwrap());
    assert!(reg.plugins_for(&other).is_empty());
}

#[test]
fn registry_clamps_limits_to_user_maxima() {
    let (_tmp, reg) = reference_registry(&UserLimits {
        max_memory_mb: 96,
        max_task_seconds: 30,
    });
    let db = reg.get(&id("arcode.db-viewer")).unwrap();
    assert_eq!(
        (db.limits.memory_bytes, db.limits.task_seconds),
        (96 * 1024 * 1024, 30)
    );
    let http = reg.get(&id("arcode.http-client")).unwrap();
    assert_eq!(
        (http.limits.memory_bytes, http.limits.task_seconds),
        (64 * 1024 * 1024, 30)
    );
    assert_eq!(http.entry_path(), http.dir.join("plugin.wasm"));
}

#[test]
fn fixture_manifests_in_test_programs_are_valid() {
    let programs = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-programs");
    let dev: Vec<PathBuf> = fs::read_dir(&programs)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("plugin.toml").is_file())
        .collect();
    assert!(
        !dev.is_empty(),
        "no fixtures found in {}",
        programs.display()
    );
    let count = dev.len();
    let reg = Registry::discover(
        &DiscoveryRoots {
            dev,
            ..DiscoveryRoots::default()
        },
        &UserLimits::default(),
    );
    assert!(reg.problems().is_empty(), "{:#?}", reg.problems());
    assert_eq!(reg.plugins().count(), count);
}
