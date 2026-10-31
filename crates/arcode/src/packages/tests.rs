//! GPUI tests of the Packages store and view.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arcode_project::deps::{DepKind, PackageAction};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};

use super::rows::Row;
use super::*;
use crate::test_support::{init, settle};

pub(super) fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// A Cargo workspace with two members and an npm package.
pub(crate) fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nserde = \"1.0\"\n",
    );
    write(
        &root,
        "crates/core/Cargo.toml",
        "[package]\nname = \"core\"\n\n[dependencies]\nserde = { workspace = true }\nanyhow = \"1\"\n\n[dev-dependencies]\ntempfile = \"3\"\n",
    );
    write(
        &root,
        "crates/cli/Cargo.toml",
        "[package]\nname = \"cli\"\n\n[dependencies]\nclap = \"4\"\n",
    );
    write(
        &root,
        "Cargo.lock",
        "version = 3\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\n\n[[package]]\nname = \"anyhow\"\nversion = \"1.0.80\"\n",
    );
    write(
        &root,
        "web/package.json",
        "{\n  \"name\": \"web\",\n  \"dependencies\": {\n    \"react\": \"^18.2.0\"\n  },\n  \"devDependencies\": {\n    \"vite\": \"^5\"\n  }\n}\n",
    );
    write(
        &root,
        "web/package-lock.json",
        "{\"packages\": {\"node_modules/react\": {\"version\": \"18.2.0\"}}}",
    );
    (dir, root)
}

pub(super) fn view_for<'a>(
    root: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<PackagesView>, &'a mut VisualTestContext) {
    init(cx);
    let store = cx.new(PackagesStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.to_path_buf()), cx));
    let (view, cx) = cx.add_window_view(|window, cx| PackagesView::new(store, window, cx));
    // The first render reads the project.
    cx.run_until_parked();
    settle(cx);
    (view, cx)
}

pub(super) fn events(
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

/// `section name` / `  dep name` lines of the view.
fn outline(view: &Entity<PackagesView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, cx| {
        let ms = v.store().read(cx).manifests();
        v.rows()
            .iter()
            .map(|row| match row {
                Row::Section(s) => format!(
                    "{} {}",
                    ms[*s].manager.label(),
                    ms[*s].name.clone().unwrap_or_else(|| ms[*s].file_name())
                ),
                Row::Dep { section, dep } => format!("  {}", ms[*section].deps[*dep].name),
                Row::Note(_) => "  (note)".to_string(),
            })
            .collect()
    })
}

#[gpui::test]
fn sections_filter_and_collapse(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (view, cx) = view_for(&root, cx);
    assert_eq!(
        outline(&view, cx),
        [
            "cargo Cargo.toml",
            "  serde",
            "cargo cli",
            "  clap",
            "cargo core",
            "  anyhow",
            "  serde",
            "  tempfile",
            "npm web",
            "  react",
            "  vite",
        ]
    );
    view.update(cx, |v, cx| v.set_filter("SER", cx));
    assert_eq!(
        outline(&view, cx),
        ["cargo Cargo.toml", "  serde", "cargo core", "  serde"]
    );
    view.update(cx, |v, cx| v.set_filter("", cx));
    view.update(cx, |v, cx| v.toggle_section(2, cx));
    assert_eq!(outline(&view, cx)[4..6], ["cargo core", "npm web"]);
    // Locked versions come from the workspace's Cargo.lock.
    let serde = view.read_with(cx, |v, cx| {
        v.store().read(cx).manifests()[2].deps[1].clone()
    });
    assert_eq!(serde.locked.as_deref(), Some("1.0.200"));
}

#[gpui::test]
fn clicking_a_dependency_opens_its_line(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (view, cx) = view_for(&root, cx);
    let seen = events(&view, cx);
    // `vite` of web/package.json, declared on line 6.
    let (section, dep) = view.read_with(cx, |v, _| match v.rows()[10] {
        Row::Dep { section, dep } => (section, dep),
        ref other => panic!("{other:?}"),
    });
    view.update(cx, |v, cx| v.open_dep(section, dep, cx));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [PackagesEvent::Open {
            path: root.join("web/package.json"),
            line: 6
        }]
    );
}

#[gpui::test]
fn row_actions_menu_and_add_form(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (view, cx) = view_for(&root, cx);
    let seen = events(&view, cx);
    let core = root.join("crates/core/Cargo.toml");
    // core's deps: anyhow, serde, tempfile (section 2).
    view.update(cx, |v, cx| v.dep_action(2, 1, DepCommand::Update, cx));
    view.update(cx, |v, cx| v.dep_action(2, 2, DepCommand::Remove, cx));
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "removing waits for confirmation"
    );
    view.update(cx, |v, cx| v.confirm_removal(cx));
    view.update(cx, |v, cx| v.dep_action(2, 0, DepCommand::Docs, cx));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [
            PackagesEvent::Run {
                manifest: core.clone(),
                action: PackageAction::Update {
                    name: "serde".into()
                }
            },
            PackagesEvent::Run {
                manifest: core.clone(),
                action: PackageAction::Remove {
                    name: "tempfile".into(),
                    kind: DepKind::Dev,
                    group: None
                }
            },
            PackagesEvent::OpenUrl("https://docs.rs/anyhow".into()),
        ]
    );
    seen.lock().unwrap().clear();

    view.update(cx, |v, cx| {
        v.open_menu(2, 0, gpui::point(gpui::px(10.), gpui::px(10.)), cx)
    });
    assert_eq!(
        view.read_with(cx, |v, cx| v.menu_labels(cx)),
        [
            "Go to Declaration",
            "Update",
            "Remove…",
            "Open Documentation"
        ]
    );

    view.update_in(cx, |v, window, cx| v.open_add(&core, window, cx));
    view.update(cx, |v, cx| {
        v.fill_add("-rf", "", false, cx);
        v.submit_add(cx);
    });
    let error = view.read_with(cx, |v, _| v.add_form().and_then(|f| f.error.clone()));
    assert!(error.is_some_and(|e| e.contains("'-'")));
    assert!(seen.lock().unwrap().is_empty(), "invalid names never run");
    view.update(cx, |v, cx| {
        v.fill_add("rand", "0.8", true, cx);
        v.submit_add(cx);
    });
    assert!(view.read_with(cx, |v, _| v.add_form().is_none()));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [PackagesEvent::Run {
            manifest: core,
            action: PackageAction::Add {
                name: "rand".into(),
                version: Some("0.8".into()),
                dev: true
            }
        }]
    );
}

#[gpui::test]
fn manifest_changes_refresh_after_a_quiet_time(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (view, cx) = view_for(&root, cx);
    let store = view.read_with(cx, |v, _| v.store().clone());
    assert_eq!(store.read_with(cx, |s, _| s.scans()), 1);
    store.update(cx, |s, cx| {
        s.paths_changed(&[root.join("crates/cli/src/main.rs")], false, cx)
    });
    settle(cx);
    assert_eq!(
        store.read_with(cx, |s, _| s.scans()),
        1,
        "not a package file"
    );

    write(
        &root,
        "crates/cli/Cargo.toml",
        "[package]\nname = \"cli\"\n\n[dependencies]\nclap = \"4\"\nregex = \"1\"\n",
    );
    for _ in 0..3 {
        store.update(cx, |s, cx| {
            s.paths_changed(&[root.join("crates/cli/Cargo.toml")], false, cx)
        });
    }
    settle(cx);
    assert_eq!(store.read_with(cx, |s, _| s.scans()), 2, "debounced");
    assert!(outline(&view, cx).contains(&"  regex".to_string()));
}

#[gpui::test]
fn requests_wait_for_the_first_read(cx: &mut TestAppContext) {
    init(cx);
    let (_dir, root) = project();
    let store = cx.new(PackagesStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.clone()), cx));
    let (view, cx) = cx.add_window_view(|window, cx| PackagesView::new(store.clone(), window, cx));
    let seen = events(&view, cx);
    let hint = root.join("web/src/main.ts");
    view.update_in(cx, |v, window, cx| {
        v.request(PackagesRequest::UpdateAll, Some(hint.clone()), window, cx);
        v.request(PackagesRequest::Add, Some(hint), window, cx);
    });
    settle(cx);
    assert_eq!(
        seen.lock().unwrap().clone(),
        [PackagesEvent::Run {
            manifest: root.join("web/package.json"),
            action: PackageAction::UpdateAll
        }]
    );
    let form = view.read_with(cx, |v, _| v.add_form().map(|f| f.manifest.clone()));
    assert_eq!(form, Some(root.join("web/package.json")));
}

#[gpui::test]
fn yarn_berry_has_no_update_check(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    write(&root, "package.json", "{\"dependencies\": {\"a\": \"1\"}}");
    write(&root, "yarn.lock", "__metadata:\n  version: 8\n");
    let store = cx.new(PackagesStore::new);
    store.update(cx, |s, cx| {
        s.set_root(Some(root.clone()), cx);
        s.ensure_loaded(cx);
    });
    settle(cx);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|cx| {
        cx.subscribe(&store, move |_, e: &PackagesStoreEvent, _| {
            sink.lock().unwrap().push(e.clone())
        })
        .detach();
    });
    store.update(cx, |s, cx| s.check_updates(&root.join("package.json"), cx));
    let seen = seen.lock().unwrap().clone();
    assert!(
        matches!(&seen[..], [PackagesStoreEvent::Message(m)] if m.contains("Yarn 2+")),
        "{seen:?}"
    );
    assert_eq!(
        store.read_with(cx, |s, _| s.check(&root.join("package.json")).cloned()),
        None
    );
}
