//! GPUI tests of installing a missing language server from the
//! workspace: the message's button, the terminal command (recorded, not
//! run, in tests) and the missing-servers list.

use std::path::PathBuf;

use arcode_lsp::{LspEvent, ServerStatus};
use arcode_project::server_install::{InstallCommandLine, InstallHint};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::open;
use crate::languages::PickPurpose;
use crate::lsp_bridge::install::{InstallLanguageServer, NotInstalled};

fn toast(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> (String, Option<String>) {
    ws.read_with(cx, |ws, _| {
        (
            ws.toast.as_ref().map(|t| t.to_string()).unwrap_or_default(),
            ws.toast_action.as_ref().map(|(label, _)| label.to_string()),
        )
    })
}

fn installing(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<String> {
    ws.read_with(cx, |ws, cx| {
        ws.lsp().read(cx).installing().map(str::to_string)
    })
}

fn fake_install() -> InstallHint {
    InstallHint::Command(InstallCommandLine {
        program: "rustup".into(),
        args: vec!["component".into(), "add".into(), "rust-analyzer".into()],
    })
}

#[gpui::test]
fn the_not_installed_message_has_an_install_button(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    ws.update(cx, |ws, cx| {
        let report = NotInstalled {
            server: "rust-analyzer".into(),
            message: "Language server rust-analyzer is not installed".into(),
            label: "Install".into(),
        };
        ws.show_not_installed(&report, cx);
        let (_, action) = ws.toast_action.as_ref().expect("a button");
        let action = action
            .as_any()
            .downcast_ref::<InstallLanguageServer>()
            .expect("the install action");
        assert_eq!(action.server.as_ref(), "rust-analyzer");
    });
    assert_eq!(
        toast(&ws, cx),
        (
            "Language server rust-analyzer is not installed".to_string(),
            Some("Install".to_string())
        )
    );
}

#[gpui::test]
fn a_failed_server_reaches_the_toast(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let lsp = ws.read_with(cx, |ws, _| ws.lsp().clone());
    lsp.update(cx, |b, cx| {
        b.handle_event(
            LspEvent::ServerStatus {
                server: "rust-analyzer".into(),
                root: PathBuf::from("/proj"),
                status: ServerStatus::Failed(
                    "rust-analyzer is not installed in the Rust toolchain stable: \
                     run `rustup component add rust-analyzer`"
                        .into(),
                ),
            },
            cx,
        )
    });
    cx.run_until_parked();
    let (text, button) = toast(&ws, cx);
    assert!(
        text.starts_with("Language server rust-analyzer is not installed"),
        "{text}"
    );
    // Install when rustup is on this machine's PATH, else the homepage.
    assert!(
        matches!(button.as_deref(), Some("Install" | "How to Install")),
        "{button:?}"
    );
}

#[gpui::test]
fn install_runs_once_and_retries_on_success(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.run_install_hint(fake_install(), window, cx)
    });
    assert_eq!(
        installing(&ws, cx).as_deref(),
        Some("rustup component add rust-analyzer")
    );
    // A second click while it runs starts nothing.
    ws.update_in(cx, |ws, window, cx| {
        ws.run_install_hint(fake_install(), window, cx)
    });
    assert_eq!(
        toast(&ws, cx).0,
        "`rustup component add rust-analyzer` is still running"
    );
    let lsp = ws.read_with(cx, |ws, _| ws.lsp().clone());
    lsp.update(cx, |b, cx| b.install_exited(Some(0), cx));
    cx.run_until_parked();
    assert_eq!(installing(&ws, cx), None);
    assert_eq!(
        toast(&ws, cx).0,
        "`rustup component add rust-analyzer` finished"
    );
}

#[gpui::test]
fn missing_servers_are_listed_to_install(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.missing_servers_found(Vec::new(), window, cx)
    });
    assert_eq!(
        toast(&ws, cx).0,
        "Every language server this project uses is installed"
    );
    ws.update_in(cx, |ws, window, cx| {
        let missing = vec![
            (
                "clangd".to_string(),
                Some(InstallHint::Link("https://clangd.llvm.org/installation")),
            ),
            ("rust-analyzer".to_string(), Some(fake_install())),
        ];
        ws.missing_servers_found(missing, window, cx);
        let Some((_, PickPurpose::InstallServer { servers })) = &ws.langs.picker else {
            panic!("no install picker");
        };
        assert_eq!(servers, &["clangd", "rust-analyzer"]);
    });
    let picker = ws.read_with(cx, |ws, _| ws.list_picker().cloned().expect("picker open"));
    picker.update(cx, |p, cx| {
        p.set_query("rust", cx);
        p.confirm(cx);
    });
    cx.run_until_parked();
    // rust-analyzer has no known hint yet: it is looked up, then run.
    let ran = installing(&ws, cx);
    assert!(
        ran.is_none() || ran.as_deref() == Some("rustup component add rust-analyzer"),
        "{ran:?}"
    );
}

#[gpui::test]
fn the_palette_command_checks_the_project(cx: &mut TestAppContext) {
    let dir = super::tests::project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    crate::test_support::settle(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.show_missing_language_servers(window, cx)
    });
    crate::test_support::settle(cx);
    // Either rust-analyzer (and taplo, for Cargo.toml) are installed here
    // and a toast says so, or the list offers them.
    ws.read_with(cx, |ws, _| match &ws.langs.picker {
        Some((_, PickPurpose::InstallServer { servers })) => {
            assert!(
                servers.iter().all(|s| s == "rust-analyzer" || s == "taplo"),
                "{servers:?}"
            );
        }
        _ => assert_eq!(
            ws.toast.as_ref().map(|t| t.to_string()).as_deref(),
            Some("Every language server this project uses is installed")
        ),
    });
}

// The Languages page looks servers up for the open project (whose
// toolchain file picks the Rust toolchain), as the server status does.
#[gpui::test]
fn the_languages_page_checks_for_the_open_project(cx: &mut TestAppContext) {
    let (_dir, root) = super::run_file_tests::loose_folder(&[("main.rs", "fn main() {}\n")]);
    let (ws, cx) = super::run_file_tests::open_folder(cx, &root, None);
    let view = ws.read_with(cx, |ws, _| ws.languages_view().clone());
    let checked =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.root().map(PathBuf::from));
    assert_eq!(checked(cx), Some(root.clone()));
    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    assert_eq!(checked(cx), None);
}
