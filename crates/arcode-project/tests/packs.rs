//! Language pack data integrity, installation checks and install plans.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use arcode_project::packs::{
    check_installed_on, check_requirements, install_plan_for, install_plan_missing,
    requirement_check,
};
use arcode_project::{
    CommandRunner, ComponentKind, InstallAction, KNOWN_LANGUAGE_IDS, Os, RunOutcome, ToolState,
    check_installed, install_plan, pack_by_id, pack_for_language, packs,
};

#[test]
fn pack_ids_and_languages_are_valid() {
    let all = packs();
    let ids: HashSet<_> = all.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), all.len(), "duplicate pack id");
    for id in ["rust", "python", "typescript", "go", "cpp", "web", "toml"] {
        assert!(ids.contains(id), "missing pack {id}");
    }
    let mut seen = HashSet::new();
    for pack in &all {
        assert!(!pack.languages.is_empty(), "{} has no languages", pack.id);
        for lang in &pack.languages {
            assert!(
                KNOWN_LANGUAGE_IDS.contains(&lang.as_str()),
                "unknown language id {lang}"
            );
            assert!(seen.insert(lang.clone()), "{lang} is served by two packs");
        }
    }
}

#[test]
fn every_component_is_checkable_and_installable_per_os() {
    for pack in packs() {
        let names: HashSet<_> = pack.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names.len(),
            pack.components.len(),
            "duplicate component in {}",
            pack.id
        );
        assert!(
            pack.components
                .iter()
                .any(|c| c.kind == ComponentKind::Lsp && !c.optional)
        );
        for c in &pack.components {
            for os in Os::ALL {
                let steps: Vec<_> = c.install_steps(os).collect();
                assert!(
                    !steps.is_empty(),
                    "{}/{} has no install step for {os:?}",
                    pack.id,
                    c.name
                );
                assert!(
                    c.check_for(os).is_some() || c.is_display_only(os),
                    "{}/{} has neither a check nor a display-only note for {os:?}",
                    pack.id,
                    c.name
                );
                for step in steps {
                    assert!(!step.description.is_empty());
                    match &step.action {
                        InstallAction::Run { program, .. } => {
                            assert!(step.network, "run steps download and must say so");
                            assert!(!program.is_empty());
                        }
                        InstallAction::Manual { instructions } => {
                            assert!(!step.network);
                            assert!(!instructions.is_empty());
                        }
                    }
                }
            }
            for req in &c.requires {
                assert!(
                    requirement_check(req, Os::Linux).is_some(),
                    "no check for requirement {req}"
                );
            }
        }
    }
}

#[test]
fn pack_lookup() {
    assert_eq!(
        pack_for_language("typescriptreact").map(|p| p.id),
        Some("typescript".into())
    );
    assert_eq!(pack_for_language("rust").map(|p| p.id), Some("rust".into()));
    assert_eq!(pack_for_language("c").map(|p| p.id), Some("cpp".into()));
    assert_eq!(pack_for_language("jsonc").map(|p| p.id), Some("web".into()));
    assert_eq!(pack_for_language("cobol"), None);
    assert_eq!(pack_by_id("go").map(|p| p.name), Some("Go".into()));
}

#[test]
fn rust_plan_lists_rustup_steps_and_lldb_notes() {
    let rust = pack_by_id("rust").expect("rust pack");
    let plan = install_plan(&rust, Os::Linux);
    let components: Vec<_> = plan.iter().map(|s| s.component.as_str()).collect();
    assert_eq!(
        components,
        ["rust-analyzer", "lldb-dap", "rustfmt", "clippy"]
    );
    assert_eq!(
        plan[0].step.command_line().as_deref(),
        Some("rustup component add rust-analyzer")
    );
    assert!(plan[0].step.network);
    assert_eq!(plan[0].requires, ["rustup"]);
    assert!(plan[1].step.is_manual());
    assert_eq!(plan[1].kind, ComponentKind::Debugger);
}

#[test]
fn python_plan_uses_os_specific_interpreter() {
    let py = pack_by_id("python").expect("python pack");
    let line = |os| {
        install_plan(&py, os)
            .into_iter()
            .find(|s| s.component == "debugpy")
            .and_then(|s| s.step.command_line())
    };
    assert_eq!(
        line(Os::Linux).as_deref(),
        Some("python3 -m pip install --user debugpy")
    );
    assert_eq!(
        line(Os::Windows).as_deref(),
        Some("python -m pip install --user debugpy")
    );
    // Optional alternatives only appear when asked for.
    assert!(
        !install_plan(&py, Os::Linux)
            .iter()
            .any(|s| s.component == "python-lsp-server")
    );
    let chosen = install_plan_for(&py, Os::MacOs, &["python-lsp-server"]);
    assert_eq!(chosen.len(), 1);
    assert_eq!(
        chosen[0].step.command_line().as_deref(),
        Some("pipx install python-lsp-server")
    );
    assert_eq!(chosen[0].requires, ["pipx"]);
}

/// Python CLI tools go through pipx (PEP 668 distro Pythons refuse
/// `pip install --user`); debugpy, a library, keeps pip with a note on Unix.
#[test]
fn python_tools_avoid_pip_user_on_externally_managed_pythons() {
    let py = pack_by_id("python").expect("python pack");
    for os in Os::ALL {
        let plan = install_plan(&py, os);
        let ruff: Vec<_> = plan.iter().filter(|s| s.component == "ruff").collect();
        assert_eq!(ruff.len(), 1);
        assert_eq!(
            ruff[0].step.command_line().as_deref(),
            Some("pipx install ruff")
        );
        let debugpy: Vec<_> = plan.iter().filter(|s| s.component == "debugpy").collect();
        let notes = debugpy.iter().filter(|s| s.step.is_manual()).count();
        assert_eq!(notes, usize::from(os != Os::Windows), "{os:?}");
    }
    let cpp = pack_by_id("cpp").expect("cpp pack");
    let clang_format = install_plan_for(&cpp, Os::Linux, &["clang-format"]);
    assert_eq!(
        clang_format[0].step.command_line().as_deref(),
        Some("pipx install clang-format")
    );
}

#[test]
fn typescript_and_go_plans() {
    let ts = pack_by_id("typescript").expect("ts pack");
    let lines: Vec<_> = install_plan(&ts, Os::Windows)
        .iter()
        .filter_map(|s| s.step.command_line())
        .collect();
    assert_eq!(
        lines,
        [
            "npm install --global typescript typescript-language-server",
            "npm install --global prettier",
            "npm install --global vscode-langservers-extracted",
        ]
    );
    let go = pack_by_id("go").expect("go pack");
    let lines: Vec<_> = install_plan(&go, Os::MacOs)
        .iter()
        .filter_map(|s| s.step.command_line())
        .collect();
    assert_eq!(
        lines,
        [
            "go install golang.org/x/tools/gopls@latest",
            "go install github.com/go-delve/delve/cmd/dlv@latest"
        ]
    );
}

/// Answers by program name and records every call.
struct Stub {
    answers: HashMap<&'static str, RunOutcome>,
    calls: Mutex<Vec<String>>,
}

impl Stub {
    fn new(answers: Vec<(&'static str, RunOutcome)>) -> Self {
        Self {
            answers: answers.into_iter().collect(),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl CommandRunner for Stub {
    fn run(&self, program: &str, args: &[String], _timeout: Duration) -> RunOutcome {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(format!("{program} {}", args.join(" ")));
        }
        self.answers
            .get(program)
            .cloned()
            .unwrap_or(RunOutcome::NotFound)
    }
}

fn success(stdout: &str) -> RunOutcome {
    RunOutcome::Success {
        stdout: stdout.to_string(),
    }
}

#[test]
fn check_installed_maps_outcomes() {
    let rust = pack_by_id("rust").expect("rust pack");
    let stub = Stub::new(vec![
        (
            "rust-analyzer",
            success("\nrust-analyzer 1.90.0 (abc 2026-01-01)\n"),
        ),
        ("rustfmt", RunOutcome::Failed { code: Some(1) }),
        ("cargo", RunOutcome::TimedOut),
    ]);
    let statuses = check_installed_on(&rust, Os::Linux, &stub, Duration::from_secs(1));
    let got: Vec<_> = statuses
        .iter()
        .map(|s| (s.component.as_str(), s.state, s.version.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "rust-analyzer",
                ToolState::Installed,
                Some("rust-analyzer 1.90.0 (abc 2026-01-01)")
            ),
            ("lldb-dap", ToolState::Missing, None),
            ("codelldb", ToolState::Unknown, None),
            ("rustfmt", ToolState::Missing, None),
            ("clippy", ToolState::Unknown, None),
        ]
    );
    let calls = stub.calls.lock().map(|c| c.clone()).unwrap_or_default();
    assert!(calls.contains(&"lldb-dap --version".to_string()));
    assert!(calls.contains(&"cargo clippy --version".to_string()));

    // macOS checks lldb-dap through xcrun.
    let mac = check_installed_on(&rust, Os::MacOs, &stub, Duration::from_secs(1));
    assert_eq!(mac.len(), rust.components.len());
    let calls = stub.calls.lock().map(|c| c.clone()).unwrap_or_default();
    assert!(calls.contains(&"xcrun --find lldb-dap".to_string()));
}

#[test]
fn missing_plan_follows_check_results() {
    let ts = pack_by_id("typescript").expect("ts pack");
    let stub = Stub::new(vec![("typescript-language-server", success("4.3.3"))]);
    let statuses = check_installed(&ts, &stub);
    let plan = install_plan_missing(&ts, Os::current(), &statuses);
    let components: Vec<_> = plan.iter().map(|s| s.component.as_str()).collect();
    // js-debug is unknown (no check), so it is not planned automatically.
    assert_eq!(components, ["prettier", "vscode-eslint-language-server"]);
}

#[test]
fn requirements_are_checked_once_each() {
    let py = pack_by_id("python").expect("python pack");
    let stub = Stub::new(vec![
        ("npm", success("10.0.0")),
        ("python3", success("Python 3.12")),
    ]);
    let got = check_requirements(&py, Os::Linux, &stub);
    assert_eq!(
        got,
        [
            ("npm".to_string(), ToolState::Installed),
            ("pipx".to_string(), ToolState::Missing),
            ("python".to_string(), ToolState::Installed)
        ]
    );
    let got = check_requirements(&py, Os::Windows, &stub);
    assert_eq!(got[2], ("python".to_string(), ToolState::Missing));
}
