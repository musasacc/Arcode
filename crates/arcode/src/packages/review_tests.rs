//! Review tests: defects found while reviewing the Packages view. Each
//! test states the expected behavior and fails on the current code.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arcode_project::deps::{DepKind, PackageAction};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::test_support::{init, settle};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn temp_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    (dir, root)
}

fn view_for<'a>(
    root: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<PackagesView>, &'a mut VisualTestContext) {
    init(cx);
    let store = cx.new(PackagesStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.to_path_buf()), cx));
    let (view, cx) = cx.add_window_view(|window, cx| PackagesView::new(store, window, cx));
    cx.run_until_parked();
    settle(cx);
    (view, cx)
}

fn events(
    view: &Entity<PackagesView>,
    cx: &mut VisualTestContext,
) -> Arc<Mutex<Vec<PackagesEvent>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, e: &PackagesEvent, _| {
            sink.lock().unwrap().push(e.clone())
        })
        .detach();
    });
    seen
}

/// `tokio` as a normal dependency and again as a dev-dependency (with
/// test features) is common. Removing the dev row asks for confirmation
/// keyed by name only, so both rows show "Remove tokio?" and the
/// confirmation removes the first `tokio` found: the normal one
/// (`cargo remove tokio` instead of `cargo remove tokio --dev`).
#[gpui::test]
fn review_removing_a_dev_dependency_with_a_normal_namesake(cx: &mut TestAppContext) {
    let (_dir, root) = temp_root();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\ntokio = \"1\"\n\n[dev-dependencies]\ntokio = { version = \"1\", features = [\"full\"] }\n",
    );
    let (view, cx) = view_for(&root, cx);
    let seen = events(&view, cx);
    let kinds = view.read_with(cx, |v, cx| {
        v.store().read(cx).manifests()[0]
            .deps
            .iter()
            .map(|d| (d.name.clone(), d.kind))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        kinds,
        [
            ("tokio".to_string(), DepKind::Normal),
            ("tokio".to_string(), DepKind::Dev)
        ]
    );
    // Remove… on the dev row, then confirm.
    view.update(cx, |v, cx| v.dep_action(0, 1, DepCommand::Remove, cx));
    view.update(cx, |v, cx| v.confirm_removal(cx));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [PackagesEvent::Run {
            manifest: root.join("Cargo.toml"),
            action: PackageAction::Remove {
                name: "tokio".into(),
                kind: DepKind::Dev,
                group: None,
            },
        }],
        "the dev-dependency row must remove the dev-dependency"
    );
}

/// The context menu remembers (section, dependency index). When the
/// project is read again while it is open (a manifest changed on disk,
/// e.g. a command finished in the terminal), its entries act on whatever
/// dependency now has that index: "Update" on the menu opened for `serde`
/// updates `anyhow`.
#[gpui::test]
fn review_context_menu_acts_on_the_dependency_it_was_opened_for(cx: &mut TestAppContext) {
    let (_dir, root) = temp_root();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\nanyhow = \"1\"\nserde = \"1\"\n",
    );
    let (view, cx) = view_for(&root, cx);
    let seen = events(&view, cx);
    // Right-click on serde (index 1).
    view.update(cx, |v, cx| {
        v.open_menu(0, 1, gpui::point(gpui::px(10.), gpui::px(10.)), cx)
    });
    // Meanwhile a dependency sorting before it is added on disk.
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\naaa = \"1\"\nanyhow = \"1\"\nserde = \"1\"\n",
    );
    let store = view.read_with(cx, |v, _| v.store().clone());
    store.update(cx, |s, cx| {
        s.paths_changed(&[root.join("Cargo.toml")], false, cx)
    });
    settle(cx);
    // The menu is still open; choose "Update".
    let menu_open = view.read_with(cx, |v, cx| !v.menu_labels(cx).is_empty());
    if menu_open {
        view.update(cx, |v, cx| v.dep_action(0, 1, DepCommand::Update, cx));
        assert_eq!(
            seen.lock().unwrap().clone(),
            [PackagesEvent::Run {
                manifest: root.join("Cargo.toml"),
                action: PackageAction::Update {
                    name: "serde".into()
                },
            }],
            "the menu opened on serde updated another package"
        );
    }
}

/// Python names are compared verbatim with the check's output. pip (and
/// uv) report `PyYAML` / `pyyaml`, `Flask-Login` / `flask-login`, while
/// manifests write `pyyaml`, `PyYAML` or `Flask_Login`: the newer version
/// is found but no "newer:" badge is shown. Names should be compared
/// PEP 503-normalized.
#[cfg(unix)]
#[gpui::test]
fn review_python_newer_badge_matches_normalized_names(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, root) = temp_root();
    write(&root, "requirements.txt", "pyyaml==6.0.1\n");
    // A project virtualenv whose `python -m pip list --outdated --format
    // json` reports PyYAML under its metadata name.
    let python = root.join(".venv/bin/python");
    write(
        &root,
        ".venv/bin/python",
        "#!/bin/sh\necho '[{\"name\": \"PyYAML\", \"version\": \"6.0.1\", \"latest_version\": \"6.0.2\", \"latest_filetype\": \"wheel\"}]'\n",
    );
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).expect("chmod");
    write(&root, ".venv/pyvenv.cfg", "home = /usr/bin\n");
    let (view, cx) = view_for(&root, cx);
    let store = view.read_with(cx, |v, _| v.store().clone());
    let manifest = root.join("requirements.txt");
    store.update(cx, |s, cx| s.check_updates(&manifest, cx));
    for _ in 0..50 {
        settle(cx);
        if store.read_with(cx, |s, _| s.check(&manifest) != Some(&CheckState::Running)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let state = store.read_with(cx, |s, _| s.check(&manifest).cloned());
    assert!(
        matches!(&state, Some(CheckState::Done(found)) if found.get("PyYAML").map(String::as_str) == Some("6.0.2")),
        "{state:?}"
    );
    let newer = view.read_with(cx, |v, cx| {
        let m = v.store().read(cx).manifests()[0].clone();
        v.newer(&m.path, &m.deps[0], cx)
    });
    assert_eq!(
        newer.as_deref(),
        Some("6.0.2"),
        "pyyaml in requirements.txt should get the 'newer: 6.0.2' badge"
    );
}
