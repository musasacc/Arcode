//! Reviewer tests: each one demonstrates a verified defect (expected to fail
//! until the implementation is fixed).

mod common;

use arcode_project::{
    RunnableKind, command_for_runnable, default_run_command, detect_run_commands,
};
use common::Project;

/// A typical C Makefile has `all` (Build) and `clean`; `clean` is classified
/// as `Run`, so the Run button deletes the build output.
#[test]
fn review_default_run_is_not_make_clean() {
    let p = Project::new();
    p.write(
        "Makefile",
        "all:\n\tcc -o app app.c\n\nclean:\n\trm -f app\n",
    );
    let cmd = default_run_command(p.root()).expect("a command");
    assert_ne!(
        cmd.args,
        vec!["clean".to_string()],
        "Run button = make clean"
    );
}

/// A library's package.json without start/dev: alphabetical order makes
/// `npm run clean` the default Run command.
#[test]
fn review_default_run_is_not_npm_clean() {
    let p = Project::new();
    p.write(
        "package.json",
        r#"{"scripts":{"build":"tsc","clean":"rm -rf dist","test":"jest"}}"#,
    );
    let cmd = default_run_command(p.root()).expect("a command");
    assert!(
        !cmd.args.iter().any(|a| a == "clean"),
        "Run button = {}",
        cmd.command_line()
    );
}

/// Lines inside a `define ... endef` block are text, not rules.
#[test]
fn review_make_define_block_is_not_a_target() {
    let p = Project::new();
    p.write(
        "Makefile",
        "define HELP\nUsage: make <target>\nendef\n\nbuild:\n\tcc main.c\n",
    );
    let targets: Vec<String> = detect_run_commands(p.root())
        .into_iter()
        .flat_map(|c| c.args)
        .collect();
    assert_eq!(targets, vec!["build".to_string()]);
}

/// `[[bin]] name = "<package>"` without `path` means `src/main.rs` in Cargo.
/// A test in `src/main.rs` must then select `--bin app`, not `--lib`.
#[test]
fn review_explicit_bin_named_like_package_maps_to_main_rs() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"app\"\n",
    );
    p.write("src/main.rs", "");
    p.write("src/lib.rs", "");
    let cmd = command_for_runnable(
        "rust",
        &p.path("src/main.rs"),
        p.root(),
        RunnableKind::Test,
        "tests::cli",
    )
    .expect("command")
    .command_line();
    assert_eq!(
        cmd,
        "cargo test --package app --bin app -- tests::cli --exact --nocapture"
    );
}

/// A `//go:build ignore` generator with `package main` next to a library
/// package does not make the root a main package; `go run .` fails there.
#[test]
fn review_go_build_ignored_main_is_not_runnable() {
    let p = Project::new();
    p.write("go.mod", "module example.com/lib\n\ngo 1.22\n");
    p.write("lib.go", "package lib\n");
    p.write(
        "gen.go",
        "//go:build ignore\n\npackage main\n\nfunc main() {}\n",
    );
    let has_go_run_dot = detect_run_commands(p.root())
        .iter()
        .any(|c| c.program == "go" && c.args == ["run", "."]);
    assert!(!has_go_run_dot);
}

/// vitest declared only in the workspace root (the usual pnpm monorepo
/// layout) is not detected for a member package: falls back to `node --test`.
#[test]
fn review_js_vitest_hoisted_to_workspace_root() {
    let p = Project::new();
    p.write(
        "package.json",
        r#"{"private":true,"devDependencies":{"vitest":"^2.0.0"}}"#,
    );
    p.write("pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    p.write("pnpm-lock.yaml", "");
    p.write("packages/a/package.json", r#"{"name":"a"}"#);
    let file = p.write("packages/a/src/x.test.ts", "");
    let cmd = command_for_runnable("typescript", &file, p.root(), RunnableKind::Test, "adds")
        .expect("command");
    assert!(
        cmd.args.iter().any(|a| a == "vitest"),
        "got {}",
        cmd.command_line()
    );
}

/// On POSIX shells an unquoted backslash is an escape: `v1\.2` typed into
/// bash becomes `v1.2`, so regex-escaped test patterns lose their escaping.
#[cfg(unix)]
#[test]
fn review_backslash_is_quoted_on_posix() {
    assert_ne!(arcode_project::quote_arg(r"v1\.2"), r"v1\.2");
}

/// A venv created on another OS (e.g. on Windows, opened from WSL) has a
/// `pyvenv.cfg` but no interpreter for this platform; the install command
/// must not point at a missing `.venv/bin/python`.
#[cfg(unix)]
#[test]
fn review_foreign_venv_interpreter_missing() {
    let p = Project::new();
    p.write(".venv/pyvenv.cfg", "home = C:\\Python312\n");
    p.write(".venv/Scripts/python.exe", "");
    let req = p.write("requirements.txt", "requests\n");
    let file = arcode_project::detect(&req).expect("detected");
    let program = std::path::Path::new(&file.install.program);
    assert!(
        !program.is_absolute() || program.is_file(),
        "install uses missing interpreter {}",
        file.install.program
    );
}
