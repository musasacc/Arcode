use super::*;
use crate::RunKind;

fn cmd(program: &str, args: &[&str], source: RunSource) -> RunCommand {
    RunCommand::new(
        program,
        args.iter().map(|s| s.to_string()).collect(),
        "/work",
        RunKind::Run,
        source,
    )
}

#[test]
fn cargo_test_builds_without_running_and_keeps_the_filter() {
    let t = debug_target(&cmd(
        "cargo",
        &[
            "test",
            "--package",
            "app",
            "--lib",
            "--",
            "tests::adds",
            "--exact",
            "--nocapture",
        ],
        RunSource::Cargo,
    ))
    .unwrap();
    let build = t.build.unwrap();
    assert_eq!(
        build.args,
        [
            "test",
            "--no-run",
            "--package",
            "app",
            "--lib",
            "--message-format=json"
        ]
    );
    assert_eq!(t.args, ["tests::adds", "--exact", "--nocapture"]);
    assert_eq!(t.language, "rust");
    assert_eq!(t.program, None);
}

#[test]
fn cargo_run_becomes_cargo_build() {
    let t = debug_target(&cmd(
        "cargo",
        &["run", "--bin", "app", "--", "--port", "1"],
        RunSource::Cargo,
    ))
    .unwrap();
    assert_eq!(
        t.build.unwrap().args,
        ["build", "--bin", "app", "--message-format=json"]
    );
    assert_eq!(t.args, ["--port", "1"]);
    assert!(debug_target(&cmd("cargo", &["clippy"], RunSource::Cargo)).is_none());
}

#[test]
fn python_scripts_and_pytest() {
    let t = debug_target(&cmd(
        "python3",
        &["-m", "pytest", "tests/test_x.py::test_a"],
        RunSource::Python,
    ))
    .unwrap();
    assert_eq!(t.module.as_deref(), Some("pytest"));
    assert_eq!(t.args, ["tests/test_x.py::test_a"]);
    let t = debug_target(&cmd("python3", &["main.py", "-v"], RunSource::Python)).unwrap();
    assert_eq!(t.program, Some(PathBuf::from("/work").join("main.py")));
    assert_eq!(t.args, ["-v"]);
}

#[test]
fn go_tests_use_delve_test_mode() {
    let t = debug_target(&cmd(
        "go",
        &["test", "-run", "^TestAdd$", "./calc"],
        RunSource::Go,
    ))
    .unwrap();
    assert_eq!(t.go_mode, Some("test"));
    assert_eq!(t.args, ["-test.run", "^TestAdd$"]);
    assert_eq!(t.program, Some(PathBuf::from("/work").join("./calc")));
    let t = debug_target(&cmd("go", &["run", "."], RunSource::Go)).unwrap();
    assert_eq!(t.go_mode, Some("debug"));
    assert!(debug_target(&cmd("npm", &["test"], RunSource::Node)).is_none());
}

#[test]
fn cargo_json_lists_executables() {
    let out = r#"{"reason":"compiler-artifact","target":{"name":"dep","kind":["lib"]},"profile":{"test":false},"executable":null,"fresh":true}
not json
{"reason":"compiler-artifact","target":{"name":"app","kind":["bin"]},"profile":{"test":true},"executable":"/w/target/debug/deps/app-123","fresh":false}
{"reason":"build-finished","success":true}"#;
    let exes = executables_from_cargo_json(out);
    assert_eq!(exes.len(), 1);
    assert_eq!(exes[0].target, "app");
    assert_eq!(exes[0].kinds, ["bin"]);
    assert!(exes[0].test);
    assert_eq!(exes[0].path, PathBuf::from("/w/target/debug/deps/app-123"));
    let lines: Vec<_> = out.lines().filter_map(cargo_message_text).collect();
    assert_eq!(lines, ["   Compiling app", "    Finished"]);
}
