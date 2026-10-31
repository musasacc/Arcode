//! GPUI tests of the Packages activity inside a workspace: palette
//! actions, opening a declaration, and the exact commands actions run.

use arcode_project::deps::PackageCommand;
use gpui::TestAppContext;

use super::tests::open;
use crate::activity_bar::Activity;
use crate::packages::{AddPackage, DepCommand, ShowPackages, UpdateAllPackages};
use crate::test_support::settle;

fn argv(c: &PackageCommand) -> String {
    let mut parts = vec![c.program.clone()];
    parts.extend(c.args.iter().cloned());
    parts.join(" ")
}

#[gpui::test]
fn actions_run_the_managers_commands_in_the_manifest_folder(cx: &mut TestAppContext) {
    let (_dir, root) = crate::packages::tests::project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    ws.update(cx, |ws, cx| {
        ws.pkgs.recorded = Some(Vec::new());
        assert!(
            !ws.pkgs.store.read(cx).is_loaded(),
            "nothing is read before the view is shown"
        );
    });

    cx.dispatch_action(ShowPackages);
    settle(cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.activity), Activity::Packages);
    let count = ws.read_with(cx, |ws, cx| ws.packages().store.read(cx).manifests().len());
    assert_eq!(count, 4);

    // core (section 2): update serde, then remove the dev dependency.
    let view = ws.read_with(cx, |ws, _| ws.packages().view.clone());
    view.update(cx, |v, cx| {
        v.dep_action(2, 1, DepCommand::Update, cx);
        v.dep_action(2, 2, DepCommand::Remove, cx);
        v.confirm_removal(cx);
    });
    cx.run_until_parked();
    let ran = ws.update(cx, |ws, _| {
        ws.pkgs.recorded.replace(Vec::new()).unwrap_or_default()
    });
    let core = root.join("crates/core");
    assert_eq!(
        ran.iter()
            .map(|c| (argv(c), c.cwd.clone()))
            .collect::<Vec<_>>(),
        [
            ("cargo update -p serde".to_string(), core.clone()),
            ("cargo remove tempfile --dev".to_string(), core.clone()),
        ]
    );

    // Palette: Update All for the active file's manifest.
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("web/package.json"), window, cx)
    });
    cx.dispatch_action(UpdateAllPackages);
    settle(cx);
    let ran = ws.update(cx, |ws, _| {
        ws.pkgs.recorded.replace(Vec::new()).unwrap_or_default()
    });
    assert_eq!(
        ran.iter()
            .map(|c| (argv(c), c.cwd.clone()))
            .collect::<Vec<_>>(),
        [(
            format!("{} update", if cfg!(windows) { "npm.cmd" } else { "npm" }),
            root.join("web")
        )]
    );

    // Palette: Add Package opens the form for that manifest.
    cx.dispatch_action(AddPackage);
    settle(cx);
    let target = view.read_with(cx, |v, _| v.add_form().map(|f| f.manifest.clone()));
    assert_eq!(target, Some(root.join("web/package.json")));
}

#[gpui::test]
fn clicking_a_dependency_opens_the_manifest_at_its_line(cx: &mut TestAppContext) {
    let (_dir, root) = crate::packages::tests::project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.dispatch_action(ShowPackages);
    settle(cx);
    let view = ws.read_with(cx, |ws, _| ws.packages().view.clone());
    // core's `tempfile`, declared on line 8 of crates/core/Cargo.toml.
    view.update(cx, |v, cx| v.open_dep(2, 2, cx));
    cx.run_until_parked();
    let (path, line) = ws.read_with(cx, |ws, cx| {
        let editor = ws.active_editor(cx).expect("editor");
        let e = editor.read(cx);
        let head = e.selections().primary().head;
        (
            e.path().map(|p| p.to_path_buf()),
            e.offset_to_point(head).line,
        )
    });
    assert_eq!(path, Some(root.join("crates/core/Cargo.toml")));
    assert_eq!(line, 8);
}

/// The root of a Cargo workspace is a virtual manifest: cargo cannot add
/// or remove there, nor edit `[workspace.dependencies]`. The view hides
/// those actions; asked anyway, nothing runs and a message says why.
#[gpui::test]
fn a_virtual_cargo_root_refuses_add_and_remove(cx: &mut TestAppContext) {
    let (_dir, root) = crate::packages::tests::project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    ws.update(cx, |ws, _| ws.pkgs.recorded = Some(Vec::new()));
    cx.dispatch_action(ShowPackages);
    settle(cx);
    let view = ws.read_with(cx, |ws, _| ws.packages().view.clone());
    // Section 0 is the workspace root; its `serde` is a workspace entry.
    view.update(cx, |v, cx| {
        v.dep_action(0, 0, DepCommand::Remove, cx);
        v.confirm_removal(cx);
    });
    cx.run_until_parked();
    let toast = ws.read_with(cx, |ws, _| ws.toast.clone().unwrap_or_default());
    assert!(toast.contains("virtual workspace"), "{toast}");
    view.update_in(cx, |v, window, cx| {
        v.open_add(&root.join("Cargo.toml"), window, cx)
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.add_form().is_none()));
    let toast = ws.read_with(cx, |ws, _| ws.toast.clone().unwrap_or_default());
    assert!(
        toast.contains("add packages to one of its members"),
        "{toast}"
    );
    let ran = ws.update(cx, |ws, _| ws.pkgs.recorded.take().unwrap_or_default());
    assert!(ran.is_empty(), "{ran:?}");
    // Updating from the root works.
    view.update(cx, |v, cx| v.dep_action(0, 0, DepCommand::Update, cx));
    cx.run_until_parked();
}

/// A check whose output cannot be read shows that output in a terminal
/// tab (printed from a temporary file) instead of running it again.
#[cfg(unix)]
#[gpui::test]
fn unreadable_check_output_is_shown_not_rerun(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    std::fs::write(root.join("requirements.txt"), "pyyaml==6.0.1\n").expect("write");
    std::fs::create_dir_all(root.join(".venv/bin")).expect("mkdir");
    std::fs::write(root.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").expect("write");
    let python = root.join(".venv/bin/python");
    std::fs::write(
        &python,
        "#!/bin/sh\necho 'not json'\necho 'boom' >&2\nexit 3\n",
    )
    .expect("write");
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    ws.update(cx, |ws, _| ws.pkgs.recorded = Some(Vec::new()));
    cx.dispatch_action(ShowPackages);
    settle(cx);
    let store = ws.read_with(cx, |ws, _| ws.packages().store.clone());
    let manifest = root.join("requirements.txt");
    store.update(cx, |s, cx| s.check_updates(&manifest, cx));
    let mut ran = Vec::new();
    for _ in 0..100 {
        settle(cx);
        ran = ws.update(cx, |ws, _| ws.pkgs.recorded.clone().unwrap_or_default());
        if !ran.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(ran.len(), 1, "{ran:?}");
    let shown = &ran[0];
    assert_eq!(shown.program, "cat", "the output is printed, not re-run");
    assert!(shown.display.ends_with("(output)"), "{}", shown.display);
    let file = std::path::PathBuf::from(&shown.args[0]);
    let text = std::fs::read_to_string(&file).expect("output file");
    let _ = std::fs::remove_file(&file);
    assert_eq!(text, "not json\nboom\n[exit code 3]\n");
}
