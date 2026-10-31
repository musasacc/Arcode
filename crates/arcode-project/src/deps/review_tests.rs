//! Review tests: defects found while reviewing the Packages model. Each
//! test states the expected behavior and fails on the current code.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use super::*;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    (dir, root)
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// `cargo remove winapi` fails for a dependency declared under
/// `[target.'cfg(windows)'.dependencies]` ("help: pass `--target
/// cfg(windows)`"). The target is known (`Dependency::group`), but the
/// command never passes it.
#[test]
fn review_cargo_remove_of_a_target_dependency_passes_the_target() {
    let (_dir, root) = project();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[target.'cfg(windows)'.dependencies]\nwinapi = \"0.3\"\n",
    );
    let m = read_manifest(&root.join("Cargo.toml"), &root).expect("manifest");
    let winapi = m.deps.iter().find(|d| d.name == "winapi").expect("winapi");
    assert_eq!(winapi.group.as_deref(), Some("cfg(windows)"));
    let action = PackageAction::Remove {
        name: winapi.name.clone(),
        kind: winapi.kind,
        group: winapi.group.clone(),
    };
    // The view drops the group of a normal dependency (`cargo remove
    // winapi`); passing it is refused ("Unexpected group name").
    let cmd = action_command(&m, &action, false)
        .unwrap_or_else(|e| panic!("no command for a target dependency: {e}"));
    assert!(
        cmd.args
            .windows(2)
            .any(|w| w == ["--target", "cfg(windows)"]),
        "cargo remove needs --target for a target dependency: {:?}",
        cmd.args
    );
}

/// The root of a Cargo workspace with several members is a virtual
/// manifest: `cargo add x` and `cargo remove x` run there fail with "no
/// package selected / could not determine which package to modify". The
/// view still offers Add… and Remove on the root section (and its
/// `[workspace.dependencies]` rows); the command should either be refused
/// up front or name a package.
#[test]
fn review_cargo_add_on_a_virtual_workspace_root_is_not_a_failing_command() {
    let (_dir, root) = project();
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nserde = \"1\"\n",
    );
    write(
        &root,
        "crates/a/Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\nserde = { workspace = true }\n",
    );
    write(&root, "crates/b/Cargo.toml", "[package]\nname = \"b\"\n");
    let found = scan_manifests(&root);
    let ws = found
        .iter()
        .find(|m| m.path == root.join("Cargo.toml"))
        .expect("root");
    assert!(ws.workspace_root);
    for action in [
        PackageAction::Add {
            name: "itoa".into(),
            version: None,
            dev: false,
        },
        PackageAction::Remove {
            name: "serde".into(),
            kind: DepKind::Normal,
            group: None,
        },
    ] {
        match action_command(ws, &action, false) {
            Err(_) => {}
            Ok(cmd) => assert!(
                cmd.args.iter().any(|a| a == "-p" || a == "--package"),
                "`cargo {}` in a virtual workspace root fails without -p",
                cmd.args.join(" ")
            ),
        }
    }
}

/// Reading a member's `Cargo.toml` also reads every ancestor `Cargo.toml`
/// (for `[workspace.dependencies]`) with `read_to_string`: no `is_file`
/// check and no size limit. A FIFO there blocks the scan forever (a
/// symlink to `/dev/zero`, which git can store, makes it allocate until
/// the IDE runs out of memory). The scan should skip non-regular files.
#[cfg(unix)]
#[test]
fn review_scan_does_not_block_on_a_special_ancestor_cargo_toml() {
    let (_dir, root) = project();
    write(
        &root,
        "crates/a/Cargo.toml",
        "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    let fifo = root.join("Cargo.toml");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo")
        .success();
    assert!(ok);
    let (tx, rx) = std::sync::mpsc::channel();
    let scan_root = root.clone();
    std::thread::spawn(move || {
        let _ = tx.send(scan_manifests(&scan_root).len());
    });
    let got = rx.recv_timeout(Duration::from_secs(5));
    if got.is_err() {
        // Unblock the scan thread so the test process can exit.
        let _ = fs::OpenOptions::new().write(true).open(&fifo);
    }
    assert!(
        got.is_ok(),
        "scan_manifests blocked on the FIFO root/Cargo.toml"
    );
}
