//! The context menu and the removal confirmation across rescans, and the
//! Add form's checks per manager.

use arcode_project::deps::{DepKind, Manager, PackageAction};

use super::forms::check_add_input;
use super::tests::{events, view_for, write};
use super::*;
use crate::test_support::settle;

/// A rescan that leaves the menu's row on the same dependency keeps the
/// menu open; a removal confirmation follows its dependency when other
/// rows move.
#[gpui::test]
fn menu_and_confirmation_follow_their_dependency(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let manifest = "[package]\nname = \"a\"\n\n[dependencies]\nanyhow = \"1\"\nserde = \"1\"\n";
    write(&root, "Cargo.toml", manifest);
    let (view, cx) = view_for(&root, cx);
    let seen = events(&view, cx);
    let store = view.read_with(cx, |v, _| v.store().clone());
    let rescan = |cx: &mut gpui::VisualTestContext| {
        store.update(cx, |s, cx| s.refresh(cx));
        settle(cx);
    };

    view.update(cx, |v, cx| {
        v.open_menu(0, 1, gpui::point(gpui::px(10.), gpui::px(10.)), cx)
    });
    write(&root, "Cargo.toml", &format!("{manifest}zstd = \"0.13\"\n"));
    rescan(cx);
    assert!(
        view.read_with(cx, |v, cx| !v.menu_labels(cx).is_empty()),
        "serde is still row 1"
    );

    // Confirm removing serde, while a dependency is added before it.
    view.update(cx, |v, cx| v.dep_action(0, 1, DepCommand::Remove, cx));
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\naaa = \"1\"\nanyhow = \"1\"\nserde = \"1\"\n",
    );
    rescan(cx);
    assert!(view.read_with(cx, |v, cx| v.menu_labels(cx).is_empty()));
    view.update(cx, |v, cx| v.confirm_removal(cx));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [PackagesEvent::Run {
            manifest: root.join("Cargo.toml"),
            action: PackageAction::Remove {
                name: "serde".into(),
                kind: DepKind::Normal,
                group: None,
            },
        }]
    );

    // A confirmation whose dependency is gone is dropped.
    view.update(cx, |v, cx| v.dep_action(0, 0, DepCommand::Remove, cx));
    write(&root, "Cargo.toml", "[package]\nname = \"a\"\n");
    rescan(cx);
    view.update(cx, |v, cx| v.confirm_removal(cx));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn add_form_checks_versions_per_manager() {
    let check = |m: Manager, version: &str| check_add_input(Some(m), "pkg", version, true);
    // Real executables on Windows take any range.
    for manager in [Manager::Cargo, Manager::Uv, Manager::Pip, Manager::Go] {
        assert!(check(manager, ">=2.31").is_ok(), "{manager:?}");
        assert!(check(manager, "^1").is_ok(), "{manager:?}");
    }
    // Batch shims would hand `^ < >` to cmd.exe.
    assert!(check(Manager::Npm, "^1").is_err());
    assert!(check(Manager::Composer, "<5").is_err());
    assert!(check_add_input(Some(Manager::Npm), "pkg", "^1", false).is_ok());
    assert!(check(Manager::Cargo, "").is_ok());
    assert!(check(Manager::Cargo, "1;x").is_err());
    assert!(check_add_input(Some(Manager::Cargo), "-rf", "", true).is_err());
}
