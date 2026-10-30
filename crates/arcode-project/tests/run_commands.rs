//! Run, test and build command detection.

mod common;

use arcode_project::{RunKind, RunSource, default_run_command, detect_run_commands};
use common::{Project, python};

fn lines(p: &Project) -> Vec<String> {
    detect_run_commands(p.root())
        .iter()
        .map(|c| c.command_line())
        .collect()
}

#[test]
fn single_cargo_package() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    p.write("src/main.rs", "fn main() {}");
    assert_eq!(lines(&p), ["cargo run", "cargo test", "cargo build"]);
    let commands = detect_run_commands(p.root());
    assert_eq!(commands[0].kind, RunKind::Run);
    assert_eq!(commands[0].source, RunSource::Cargo);
    assert_eq!(commands[0].cwd, p.root());
    assert_eq!(commands[0].label, "cargo run");
    assert_eq!(commands[1].kind, RunKind::Test);
    assert_eq!(commands[2].kind, RunKind::Build);
}

#[test]
fn cargo_package_with_several_bins() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\n\n[[bin]]\nname = \"server\"\npath = \"src/server.rs\"\n",
    );
    p.write("src/main.rs", "fn main() {}");
    p.write("src/bin/tool.rs", "fn main() {}");
    p.write("src/bin/multi/main.rs", "fn main() {}");
    assert_eq!(
        lines(&p),
        [
            "cargo run --bin server",
            "cargo run --bin app",
            "cargo run --bin multi",
            "cargo run --bin tool",
            "cargo test",
            "cargo build"
        ]
    );
}

#[test]
fn cargo_default_run_is_plain_cargo_run() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\ndefault-run = \"tool\"\n",
    );
    p.write("src/main.rs", "");
    p.write("src/bin/tool.rs", "");
    assert_eq!(
        lines(&p),
        [
            "cargo run",
            "cargo run --bin app",
            "cargo test",
            "cargo build"
        ]
    );
}

#[test]
fn cargo_library_has_no_run_and_defaults_to_test() {
    let p = Project::new();
    p.write("Cargo.toml", "[package]\nname = \"lib\"\n");
    p.write("src/lib.rs", "");
    assert_eq!(lines(&p), ["cargo test", "cargo build"]);
    let default = default_run_command(p.root()).expect("default");
    assert_eq!(default.command_line(), "cargo test");
}

#[test]
fn cargo_workspace_members() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\", \"tools/cli\"]\nexclude = [\"crates/skip\"]\n",
    );
    p.write("crates/core/Cargo.toml", "[package]\nname = \"core\"\n");
    p.write("crates/core/src/lib.rs", "");
    p.write("crates/app/Cargo.toml", "[package]\nname = \"my-app\"\n");
    p.write("crates/app/src/main.rs", "");
    p.write("crates/app/src/bin/helper.rs", "");
    p.write("crates/skip/Cargo.toml", "[package]\nname = \"skip\"\n");
    p.write("crates/skip/src/main.rs", "");
    p.mkdir("crates/not-a-crate");
    p.write(
        "tools/cli/Cargo.toml",
        "[package]\nname = \"cli\"\nautobins = false\n[[bin]]\nname = \"x\"\n",
    );
    p.write("tools/cli/src/main.rs", "");
    assert_eq!(
        lines(&p),
        [
            "cargo run -p my-app --bin my-app",
            "cargo run -p my-app --bin helper",
            "cargo run -p cli --bin x",
            "cargo test --workspace",
            "cargo build --workspace"
        ]
    );
}

#[test]
fn cargo_workspace_with_root_package() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"root\"\n[workspace]\nmembers = [\"sub\"]\n",
    );
    p.write("src/main.rs", "");
    p.write("sub/Cargo.toml", "[package]\nname = \"sub\"\n");
    p.write("sub/src/main.rs", "");
    assert_eq!(
        lines(&p)[..2],
        ["cargo run -p root --bin root", "cargo run -p sub --bin sub"]
    );
}

#[test]
fn package_json_scripts() {
    let p = Project::new();
    p.write(
        "package.json",
        r#"{"scripts": {"lint": "eslint .", "build": "vite build", "test": "vitest",
            "dev": "vite", "start": "node server.js", "test:e2e": "playwright test"}}"#,
    );
    p.write("pnpm-lock.yaml", "");
    let commands = detect_run_commands(p.root());
    let got: Vec<_> = commands
        .iter()
        .map(|c| (c.label.as_str(), c.kind))
        .collect();
    assert_eq!(
        got,
        [
            ("pnpm run start", RunKind::Run),
            ("pnpm run dev", RunKind::Run),
            ("pnpm run build", RunKind::Build),
            ("pnpm run lint", RunKind::Run),
            ("pnpm run test", RunKind::Test),
            ("pnpm run test:e2e", RunKind::Test),
        ]
    );
    assert!(
        commands
            .iter()
            .all(|c| c.source == RunSource::Node && c.program == "pnpm")
    );
    assert_eq!(
        default_run_command(p.root()).map(|c| c.label),
        Some("pnpm run start".into())
    );
}

#[test]
fn go_module() {
    let p = Project::new();
    p.write("go.mod", "module example.com/x\n");
    p.write("main.go", "// comment\npackage main\n\nfunc main() {}\n");
    p.write("main_test.go", "package main\n");
    p.write("cmd/tool/main.go", "package main\n");
    p.write("cmd/lib/lib.go", "package lib\n");
    p.write("internal/a/a.go", "package a\n");
    assert_eq!(
        lines(&p),
        [
            "go run .",
            "go run ./cmd/tool",
            "go test ./...",
            "go build ./..."
        ]
    );
}

#[test]
fn go_library_module() {
    let p = Project::new();
    p.write("go.mod", "module example.com/x\n");
    p.write("lib.go", "package x\n");
    assert_eq!(lines(&p), ["go test ./...", "go build ./..."]);
}

#[test]
fn python_entry_points_and_pytest() {
    let p = Project::new();
    p.write("main.py", "");
    p.write("manage.py", "");
    p.write("tests/test_app.py", "");
    let commands = detect_run_commands(p.root());
    let labels: Vec<_> = commands.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "python main.py",
            "python manage.py runserver",
            "python -m pytest"
        ]
    );
    assert_eq!(commands[0].program, python());
    assert_eq!(
        commands[1].command_line(),
        format!("{} manage.py runserver", python())
    );
    assert_eq!(commands[2].kind, RunKind::Test);
}

#[test]
fn python_uses_venv_interpreter() {
    let p = Project::new();
    let interpreter = p.venv(".venv");
    p.write("app.py", "");
    let commands = detect_run_commands(p.root());
    assert_eq!(commands[0].program, interpreter.to_string_lossy());
    assert_eq!(commands[0].args, ["app.py"]);
}

#[test]
fn pytest_detection() {
    let configured = Project::new();
    configured.write(
        "pyproject.toml",
        "[tool.pytest.ini_options]\naddopts = \"-q\"\n",
    );
    assert_eq!(lines(&configured), [format!("{} -m pytest", python())]);

    let ini = Project::new();
    ini.write("pytest.ini", "[pytest]\n");
    assert_eq!(lines(&ini).len(), 1);

    // A Rust tests/ directory is not a pytest suite.
    let rust = Project::new();
    rust.write("tests/integration.rs", "");
    assert!(lines(&rust).is_empty());
}

#[test]
fn makefile_and_justfile() {
    let p = Project::new();
    p.write(
        "Makefile",
        ".PHONY: all\nall: build\nbuild:\n\tcc main.c\ntest: build\n\t./t\nVAR := 1\n",
    );
    p.write(
        "justfile",
        "default:\n  just --list\nserve port='8080':\n  python -m http.server\n_private:\n  true\n",
    );
    let commands = detect_run_commands(p.root());
    let got: Vec<_> = commands
        .iter()
        .map(|c| (c.command_line(), c.kind, c.source))
        .collect();
    assert_eq!(
        got,
        [
            ("just default".to_string(), RunKind::Run, RunSource::Just),
            ("just serve".to_string(), RunKind::Run, RunSource::Just),
            ("make all".to_string(), RunKind::Build, RunSource::Make),
            ("make build".to_string(), RunKind::Build, RunSource::Make),
            ("make test".to_string(), RunKind::Test, RunSource::Make),
        ]
    );
}

#[test]
fn mixed_project_default_prefers_cargo() {
    let p = Project::new();
    p.write("Cargo.toml", "[package]\nname = \"app\"\n");
    p.write("src/main.rs", "");
    p.write("package.json", r#"{"scripts": {"dev": "vite"}}"#);
    p.write("Makefile", "release:\n\tcargo build --release\n");
    let default = default_run_command(p.root()).expect("default");
    assert_eq!(default.command_line(), "cargo run");
    assert!(lines(&p).contains(&"npm run dev".to_string()));
    assert!(lines(&p).contains(&"make release".to_string()));
}

#[test]
fn empty_project() {
    let p = Project::new();
    assert!(detect_run_commands(p.root()).is_empty());
    assert_eq!(default_run_command(p.root()), None);
}

#[test]
fn invalid_manifests_are_ignored() {
    let p = Project::new();
    p.write("Cargo.toml", "not = [valid");
    p.write("package.json", "{ nope");
    assert!(detect_run_commands(p.root()).is_empty());
}

/// Housekeeping names are listed but never become the Run button.
#[test]
fn default_run_skips_housekeeping() {
    let p = Project::new();
    p.write(
        "Makefile",
        "clean:\n\trm -rf out\ninstall:\n\tcp app /usr/local/bin\n",
    );
    assert_eq!(detect_run_commands(p.root()).len(), 2);
    assert_eq!(default_run_command(p.root()), None);

    let q = Project::new();
    q.write(
        "package.json",
        r#"{"scripts":{"clean":"rm -rf dist","lint":"eslint .","prestart":"x","serve":"node s.js"}}"#,
    );
    q.write("package-lock.json", "{}");
    let cmd = default_run_command(q.root()).expect("a command");
    assert_eq!(cmd.command_line(), "npm run serve");
}

/// A `go:generate` helper does not hide a real main package under `cmd/`.
#[test]
fn go_ignored_generator_in_main_package_dir() {
    let p = Project::new();
    p.write("go.mod", "module example.com/app\n\ngo 1.22\n");
    p.write(
        "gen.go",
        "// +build ignore\n\npackage main\n\nfunc main() {}\n",
    );
    p.write(
        "main.go",
        "//go:build !windows\n\npackage main\n\nfunc main() {}\n",
    );
    let runs: Vec<String> = detect_run_commands(p.root())
        .iter()
        .filter(|c| c.kind == RunKind::Run)
        .map(|c| c.command_line())
        .collect();
    assert_eq!(runs, ["go run ."]);
}
