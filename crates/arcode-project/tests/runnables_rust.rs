//! Commands for Rust runnables.

mod common;

use arcode_project::{RunKind, RunSource, Runnable, RunnableKind, command_for_runnable};
use common::Project;

fn line(p: &Project, file: &str, kind: RunnableKind, name: &str) -> Option<String> {
    command_for_runnable("rust", &p.path(file), p.root(), kind, name).map(|c| c.command_line())
}

fn rust_project() -> Project {
    let p = Project::new();
    p.write("Cargo.toml", "[package]\nname = \"foo\"\n");
    p.write("src/lib.rs", "");
    p.write("src/main.rs", "");
    p.write("src/net/mod.rs", "");
    p.write("src/net/tcp.rs", "");
    p.write("src/bin/tool.rs", "");
    p.write("tests/integration.rs", "");
    p.write("benches/speed.rs", "");
    p.write("examples/demo.rs", "");
    p
}

#[test]
fn rust_tests() {
    let p = rust_project();
    let test = |file, name| line(&p, file, RunnableKind::Test, name);
    assert_eq!(
        test("src/lib.rs", "tests::it_works").as_deref(),
        Some("cargo test --package foo --lib -- tests::it_works --exact --nocapture")
    );
    assert_eq!(
        test("src/net/tcp.rs", "tests::connects").as_deref(),
        Some("cargo test --package foo --lib -- net::tcp::tests::connects --exact --nocapture")
    );
    assert_eq!(
        test("src/net/mod.rs", "parses").as_deref(),
        Some("cargo test --package foo --lib -- net::parses --exact --nocapture")
    );
    assert_eq!(
        test("tests/integration.rs", "round_trip").as_deref(),
        Some("cargo test --package foo --test integration -- round_trip --exact --nocapture")
    );
    assert_eq!(
        test("src/main.rs", "tests::cli").as_deref(),
        Some("cargo test --package foo --bin foo -- tests::cli --exact --nocapture")
    );
    assert_eq!(
        test("src/bin/tool.rs", "t").as_deref(),
        Some("cargo test --package foo --bin tool -- t --exact --nocapture")
    );
}

#[test]
fn rust_test_module_and_bench() {
    let p = rust_project();
    assert_eq!(
        line(&p, "src/net/tcp.rs", RunnableKind::TestModule, "tests").as_deref(),
        Some("cargo test --package foo --lib -- net::tcp::tests --nocapture")
    );
    assert_eq!(
        line(&p, "tests/integration.rs", RunnableKind::TestModule, "").as_deref(),
        Some("cargo test --package foo --test integration -- --nocapture")
    );
    assert_eq!(
        line(&p, "benches/speed.rs", RunnableKind::Benchmark, "parse").as_deref(),
        Some("cargo bench --package foo --bench speed -- parse --exact")
    );
}

#[test]
fn rust_mains() {
    let p = rust_project();
    let main = |file| line(&p, file, RunnableKind::Main, "main");
    assert_eq!(
        main("src/main.rs").as_deref(),
        Some("cargo run --package foo --bin foo")
    );
    assert_eq!(
        main("src/bin/tool.rs").as_deref(),
        Some("cargo run --package foo --bin tool")
    );
    assert_eq!(
        main("examples/demo.rs").as_deref(),
        Some("cargo run --package foo --example demo")
    );
    let cmd = command_for_runnable(
        "rust",
        &p.path("src/main.rs"),
        p.root(),
        RunnableKind::Main,
        "main",
    )
    .expect("command");
    assert_eq!(
        (cmd.kind, cmd.source, cmd.label.as_str()),
        (RunKind::Run, RunSource::Cargo, "run")
    );
}

#[test]
fn rust_workspace_member_uses_member_package() {
    let p = Project::new();
    p.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    p.write("crates/core/Cargo.toml", "[package]\nname = \"arc-core\"\n");
    p.write("crates/core/src/lib.rs", "");
    p.write("crates/core/src/a.rs", "");
    let cmd = command_for_runnable(
        "rust",
        &p.path("crates/core/src/a.rs"),
        p.root(),
        RunnableKind::Test,
        "x",
    )
    .expect("command");
    assert_eq!(
        cmd.command_line(),
        "cargo test --package arc-core --lib -- a::x --exact --nocapture"
    );
    assert_eq!(cmd.cwd, p.root());
    assert_eq!(cmd.label, "test a::x");
}

#[test]
fn rust_without_cargo_has_no_command() {
    let p = Project::new();
    p.write("x.rs", "");
    assert_eq!(line(&p, "x.rs", RunnableKind::Test, "t"), None);
}

#[test]
fn runnable_new_carries_line_and_command() {
    let p = rust_project();
    let r = Runnable::new(
        "rust",
        &p.path("src/lib.rs"),
        p.root(),
        41,
        RunnableKind::Test,
        "tests::ok",
    )
    .expect("runnable");
    assert_eq!(
        (r.line, r.kind, r.name.as_str()),
        (41, RunnableKind::Test, "tests::ok")
    );
    assert_eq!(r.command.kind, RunKind::Test);
    assert_eq!(r.command.label, "test tests::ok");
}

/// `[[bin]]` without `path` resolves like Cargo: `src/bin/<name>/main.rs`
/// when that exists, `src/bin/<name>.rs` otherwise.
#[test]
fn explicit_bins_without_path_are_inferred() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nautobins = false\n\n\
         [[bin]]\nname = \"tool\"\n\n[[bin]]\nname = \"other\"\n",
    );
    p.write("src/lib.rs", "");
    let tool = p.write("src/bin/tool/main.rs", "");
    let other = p.write("src/bin/other.rs", "");
    for (file, bin) in [(tool, "tool"), (other, "other")] {
        let cmd = command_for_runnable("rust", &file, p.root(), RunnableKind::Main, "main")
            .expect("command");
        assert!(
            cmd.args.windows(2).any(|w| w[0] == "--bin" && w[1] == bin),
            "{}",
            cmd.command_line()
        );
    }
}
