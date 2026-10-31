use super::*;

const SAMPLE: &str = r#"
[[configuration]]
name = "Debug server"
adapter = "lldb-dap"
program = "target/debug/server"
args = ["--port", "${env:ARCODE_TEST_PORT}"]
env = { RUST_LOG = "debug" }
cargo = ["build", "--bin", "server"]

[[configuration]]
name = "Tests"
language = "python"
module = "pytest"
args = ["${file}"]
[configuration.extra]
justMyCode = false

[[configuration]]
name = "Attach"
adapter = "gdb"
request = "attach"
pid = 4242

[[configuration]]
name = "Custom"
program = "/bin/app"
[configuration.adapter_command]
command = "my-dap"
args = ["--port", "{port}"]
transport = "tcp"
"#;

fn vars() -> LaunchVars {
    LaunchVars {
        workspace_folder: PathBuf::from("/work/app"),
        file: Some(PathBuf::from("/work/app/tests/test_x.py")),
    }
}

#[test]
fn parses_every_kind_of_entry() {
    let (entries, warnings) = parse_launch_file(SAMPLE);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries[0].adapter,
        AdapterChoice::Known(KnownAdapter::LldbDap)
    );
    assert_eq!(
        entries[0].cargo.as_deref(),
        Some(&["build".to_string(), "--bin".into(), "server".into()][..])
    );
    assert_eq!(entries[1].adapter, AdapterChoice::Language("python".into()));
    assert_eq!(entries[2].request, DebugRequest::Attach);
    assert!(matches!(entries[3].adapter, AdapterChoice::Custom(_)));
}

#[test]
fn launch_arguments_follow_the_adapter() {
    let (entries, _) = parse_launch_file(SAMPLE);
    // SAFETY: tests in this binary do not read this variable concurrently.
    unsafe { std::env::set_var("ARCODE_TEST_PORT", "8080") };
    let e = &entries[0];
    let config = e.to_config(
        KnownAdapter::LldbDap.spec(),
        Some(KnownAdapter::LldbDap),
        &vars(),
        None,
    );
    let expected = PathBuf::from("/work/app").join("target/debug/server");
    assert_eq!(
        config.arguments["program"],
        json!(expected.to_string_lossy())
    );
    assert_eq!(config.arguments["args"], json!(["--port", "8080"]));
    assert_eq!(config.arguments["env"], json!(["RUST_LOG=debug"]));
    assert_eq!(config.arguments["cwd"], "/work/app");

    let py = entries[1].to_config(
        KnownAdapter::Debugpy.spec(),
        Some(KnownAdapter::Debugpy),
        &vars(),
        None,
    );
    assert!(py.arguments.get("program").is_none());
    assert_eq!(py.arguments["module"], "pytest");
    assert_eq!(py.arguments["args"], json!(["/work/app/tests/test_x.py"]));
    assert_eq!(py.arguments["justMyCode"], false);

    let attach = entries[2].to_config(
        KnownAdapter::Gdb.spec(),
        Some(KnownAdapter::Gdb),
        &vars(),
        None,
    );
    assert_eq!(attach.request, DebugRequest::Attach);
    assert_eq!(attach.arguments, json!({ "pid": 4242 }));
}

#[test]
fn attach_uses_each_adapters_process_id_key() {
    let (entries, warnings) = parse_launch_file(
        "[[configuration]]\nname = \"a\"\nadapter = \"delve\"\nrequest = \"attach\"\npid = 1234\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let attach = |known: KnownAdapter| {
        entries[0]
            .to_config(known.spec(), Some(known), &vars(), None)
            .arguments
    };
    assert_eq!(
        attach(KnownAdapter::Delve),
        json!({ "processId": 1234, "mode": "local" })
    );
    assert_eq!(attach(KnownAdapter::Debugpy), json!({ "processId": 1234 }));
    assert_eq!(attach(KnownAdapter::LldbDap), json!({ "pid": 1234 }));
}

#[test]
fn a_built_program_replaces_the_configured_one() {
    let (entries, _) = parse_launch_file(SAMPLE);
    let config = entries[0].to_config(
        KnownAdapter::Gdb.spec(),
        Some(KnownAdapter::Gdb),
        &vars(),
        Some(PathBuf::from("/work/app/target/debug/deps/server-abc")),
    );
    assert_eq!(
        config.arguments["program"],
        "/work/app/target/debug/deps/server-abc"
    );
}

#[test]
fn broken_entries_are_skipped_with_a_named_warning() {
    let text = r#"
[[configuration]]
name = "ok"
program = "a.py"

[[configuration]]
name = "bad adapter"
adapter = "nope"
program = "x"

[[configuration]]
name = "no program"

[[configuration]]
nam = "typo"
"#;
    let (entries, warnings) = parse_launch_file(text);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].adapter, AdapterChoice::Language("python".into()));
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings[0].contains("`bad adapter`") && warnings[0].contains("unknown adapter"));
    assert!(warnings[1].contains("set `program`"));
    assert!(warnings[2].contains("#4"));
    let (none, warnings) = parse_launch_file("[[configuration]\n");
    assert!(none.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn variables_are_substituted() {
    let v = vars();
    assert_eq!(v.substitute("${workspaceFolderBasename}"), "app");
    assert_eq!(v.substitute("${fileBasenameNoExtension}.out"), "test_x.out");
    assert_eq!(v.substitute("${fileDirname}"), "/work/app/tests");
    assert_eq!(
        v.substitute("keep ${unknown} and ${"),
        "keep ${unknown} and ${"
    );
    assert_eq!(v.path("bin/x"), PathBuf::from("/work/app").join("bin/x"));
    let no_file = LaunchVars {
        file: None,
        ..vars()
    };
    assert_eq!(no_file.substitute("[${file}]"), "[]");
}
