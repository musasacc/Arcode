//! Builders for the built-in pack data: components, checks, install steps.

use super::{CheckCommand, Component, ComponentKind, InstallAction, InstallStep, LanguagePack, Os};
use crate::command::args;

const UNIX: [Os; 2] = [Os::Linux, Os::MacOs];

pub(super) fn lldb_dap() -> Component {
    let checks = vec![
        check_on(&[Os::Linux, Os::Windows], "lldb-dap", &["--version"]),
        check_on(&[Os::MacOs], "xcrun", &["--find", "lldb-dap"]),
    ];
    comp(
        ComponentKind::Debugger,
        "lldb-dap",
        checks,
        vec![
            note(
                &[Os::Linux],
                "Install LLDB 18 or newer from your distribution (`sudo apt install lldb`, \
             `sudo dnf install lldb`); it provides lldb-dap (on Debian/Ubuntu possibly as lldb-dap-<version>).",
            ),
            note(
                &[Os::MacOs],
                "Install the Xcode Command Line Tools (`xcode-select --install`); lldb-dap is then \
             available through `xcrun lldb-dap`.",
            ),
            note(
                &[Os::Windows],
                "Install LLVM (`winget install LLVM.LLVM`) and add C:\\Program Files\\LLVM\\bin to PATH.",
            ),
        ],
        &[],
    )
}

pub(super) fn codelldb() -> Component {
    optional(comp(
        ComponentKind::Debugger,
        "codelldb",
        Vec::new(),
        vec![note(
            &Os::ALL,
            "Download the CodeLLDB package for your platform from https://github.com/vadimcn/codelldb/releases, \
         extract it, and put its adapter/codelldb executable on PATH.",
        )],
        &[],
    ))
}

pub(super) fn pack(
    id: &str,
    name: &str,
    languages: &[&str],
    components: Vec<Component>,
) -> LanguagePack {
    LanguagePack {
        id: id.to_string(),
        name: name.to_string(),
        languages: args(languages),
        components,
    }
}

pub(super) fn comp(
    kind: ComponentKind,
    name: &str,
    checks: Vec<CheckCommand>,
    install: Vec<InstallStep>,
    requires: &[&str],
) -> Component {
    Component {
        kind,
        name: name.to_string(),
        checks,
        install,
        requires: args(requires),
        optional: false,
    }
}

pub(super) fn optional(mut component: Component) -> Component {
    component.optional = true;
    component
}

pub(super) fn check(program: &str, list: &[&str]) -> CheckCommand {
    check_on(&Os::ALL, program, list)
}

pub(super) fn check_on(os: &[Os], program: &str, list: &[&str]) -> CheckCommand {
    CheckCommand {
        os: os.to_vec(),
        program: program.to_string(),
        args: args(list),
    }
}

/// `python -c "import <module>"` per OS.
pub(super) fn python_import_check(module: &str) -> Vec<CheckCommand> {
    let code = format!("import {module}");
    vec![
        check_on(&UNIX, Os::Linux.python(), &["-c", &code]),
        check_on(&[Os::Windows], Os::Windows.python(), &["-c", &code]),
    ]
}

/// `npm ls -g` exits non-zero when the package is missing; the servers in
/// this package have no `--version` flag.
pub(super) fn langservers_extracted_check() -> CheckCommand {
    check(
        "npm",
        &[
            "ls",
            "--global",
            "--depth=0",
            "vscode-langservers-extracted",
        ],
    )
}

pub(super) fn run(os: &[Os], description: &str, program: &str, list: &[&str]) -> InstallStep {
    InstallStep {
        os: os.to_vec(),
        description: description.to_string(),
        action: InstallAction::Run {
            program: program.to_string(),
            args: args(list),
        },
        network: true,
    }
}

pub(super) fn note(os: &[Os], instructions: &str) -> InstallStep {
    InstallStep {
        os: os.to_vec(),
        description: "Manual installation (Arcode does not run anything).".to_string(),
        action: InstallAction::Manual {
            instructions: instructions.to_string(),
        },
        network: false,
    }
}

pub(super) fn rustup_component(component: &str) -> InstallStep {
    let description =
        format!("Add the {component} component with rustup (downloads from static.rust-lang.org).");
    run(
        &Os::ALL,
        &description,
        "rustup",
        &["component", "add", component],
    )
}

pub(super) fn npm_global(packages: &[&str]) -> InstallStep {
    let description = format!(
        "Install {} globally with npm (downloads from the npm registry).",
        packages.join(", ")
    );
    let mut list = vec!["install", "--global"];
    list.extend_from_slice(packages);
    run(&Os::ALL, &description, "npm", &list)
}

pub(super) fn go_install(module: &str) -> InstallStep {
    let description =
        format!("Build and install {module} with go install (downloads from the Go module proxy).");
    run(&Os::ALL, &description, "go", &["install", module])
}

/// `python -m pip install --user <package>` for a library that the
/// interpreter must import (debugpy), with the right executable per OS.
///
/// On Linux and macOS the system or Homebrew Python is often "externally
/// managed" (PEP 668) and refuses `--user` installs, and inside an active
/// virtualenv user site-packages are invisible; a display-only note says
/// what to do then.
pub(super) fn pip_user(package: &str) -> Vec<InstallStep> {
    let description =
        format!("Install {package} for the current user with pip (downloads from PyPI).");
    let list = ["-m", "pip", "install", "--user", package];
    let unix_python = Os::Linux.python();
    vec![
        run(&UNIX, &description, unix_python, &list),
        note(
            &UNIX,
            &format!(
                "If pip reports `externally-managed-environment`, or you use a virtual \
                 environment, install {package} into the project's environment instead \
                 (`.venv/bin/python -m pip install {package}`) or use your distribution's \
                 package (e.g. `sudo apt install python3-{package}`)."
            ),
        ),
        run(&[Os::Windows], &description, Os::Windows.python(), &list),
    ]
}

/// `pipx install <package>`: a Python command-line tool in its own
/// environment, which works with externally managed (PEP 668) Pythons.
pub(super) fn pipx(package: &str) -> InstallStep {
    let description = format!(
        "Install {package} with pipx into its own environment (downloads from PyPI); \
         run `pipx ensurepath` once if its commands are not found afterwards."
    );
    run(&Os::ALL, &description, "pipx", &["install", package])
}
