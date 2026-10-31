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
