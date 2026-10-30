//! Commands for Go, Python and JS/TS runnables.

mod common;

use arcode_project::{RunnableKind, command_for_runnable};
use common::{Project, python};

fn line(p: &Project, lang: &str, file: &str, kind: RunnableKind, name: &str) -> Option<String> {
    command_for_runnable(lang, &p.path(file), p.root(), kind, name).map(|c| c.command_line())
}

#[test]
fn go_commands() {
    let p = Project::new();
    p.write("go.mod", "module x\n");
    p.write("pkg/util/util_test.go", "");
    p.write("root_test.go", "");
    p.write("cmd/app/main.go", "");
    assert_eq!(
        line(
            &p,
            "go",
            "pkg/util/util_test.go",
            RunnableKind::Test,
            "TestParse"
        )
        .as_deref(),
        Some("go test -run '^TestParse$' ./pkg/util")
    );
    assert_eq!(
        line(
            &p,
            "go",
            "root_test.go",
            RunnableKind::Test,
            "TestA/with space"
        )
        .as_deref(),
        Some("go test -run '^TestA$/^with_space$' .")
    );
    assert_eq!(
        line(
            &p,
            "go",
            "pkg/util/util_test.go",
            RunnableKind::Benchmark,
            "BenchmarkX"
        )
        .as_deref(),
        Some("go test -run '^$' -bench '^BenchmarkX$' ./pkg/util")
    );
    assert_eq!(
        line(
            &p,
            "go",
            "pkg/util/util_test.go",
            RunnableKind::TestModule,
            ""
        )
        .as_deref(),
        Some("go test ./pkg/util")
    );
    assert_eq!(
        line(&p, "go", "cmd/app/main.go", RunnableKind::Main, "main").as_deref(),
        Some("go run ./cmd/app")
    );
}

#[test]
fn go_nested_module_is_the_cwd() {
    let p = Project::new();
    p.write("services/api/go.mod", "module api\n");
    p.write("services/api/h/h_test.go", "");
    let cmd = command_for_runnable(
        "go",
        &p.path("services/api/h/h_test.go"),
        p.root(),
        RunnableKind::Test,
        "TestH",
    )
    .expect("command");
    assert_eq!(cmd.cwd, p.path("services/api"));
    assert_eq!(cmd.args.last().map(String::as_str), Some("./h"));
}

#[test]
fn python_commands() {
    let p = Project::new();
    p.write("tests/test_api.py", "");
    p.write("main.py", "");
    let py = python();
    assert_eq!(
        line(
            &p,
            "python",
            "tests/test_api.py",
            RunnableKind::Test,
            "test_get"
        ),
        Some(format!("{py} -m pytest tests/test_api.py::test_get"))
    );
    assert_eq!(
        line(
            &p,
            "python",
            "tests/test_api.py",
            RunnableKind::Test,
            "TestApi.test_post"
        ),
        Some(format!(
            "{py} -m pytest tests/test_api.py::TestApi::test_post"
        ))
    );
    assert_eq!(
        line(
            &p,
            "python",
            "tests/test_api.py",
            RunnableKind::TestModule,
            ""
        ),
        Some(format!("{py} -m pytest tests/test_api.py"))
    );
    assert_eq!(
        line(&p, "python", "main.py", RunnableKind::Main, ""),
        Some(format!("{py} main.py"))
    );
}

#[test]
fn python_uses_venv() {
    let p = Project::new();
    let interpreter = p.venv(".venv");
    p.write("t/test_x.py", "");
    let cmd = command_for_runnable(
        "python",
        &p.path("t/test_x.py"),
        p.root(),
        RunnableKind::Test,
        "test_a",
    )
    .expect("command");
    assert_eq!(cmd.program, interpreter.to_string_lossy());
    assert_eq!(cmd.args, ["-m", "pytest", "t/test_x.py::test_a"]);
}

#[test]
fn js_vitest_and_jest() {
    let p = Project::new();
    p.write(
        "web/package.json",
        r#"{"devDependencies": {"vitest": "^2"}}"#,
    );
    p.write("web/src/a.test.ts", "");
    let cmd = command_for_runnable(
        "typescript",
        &p.path("web/src/a.test.ts"),
        p.root(),
        RunnableKind::Test,
        "adds (1+1)",
    )
    .expect("command");
    assert_eq!(
        cmd.command_line(),
        r"npx --no-install vitest run src/a.test.ts -t 'adds \(1\+1\)'"
    );
    assert_eq!(cmd.cwd, p.path("web"));
    assert_eq!(
        line(
            &p,
            "typescript",
            "web/src/a.test.ts",
            RunnableKind::Benchmark,
            "sort"
        )
        .as_deref(),
        Some("npx --no-install vitest bench --run src/a.test.ts -t sort")
    );

    let jest = Project::new();
    jest.write("package.json", r#"{"devDependencies": {"jest": "29"}}"#);
    jest.write("yarn.lock", "");
    assert_eq!(
        line(
            &jest,
            "javascript",
            "a.test.js",
            RunnableKind::TestModule,
            "math"
        )
        .as_deref(),
        Some("yarn jest a.test.js -t math")
    );
    assert_eq!(
        line(
            &jest,
            "javascript",
            "a.test.js",
            RunnableKind::Benchmark,
            "x"
        ),
        None
    );
}

#[test]
fn js_pnpm_exec_and_node_test_fallback() {
    let p = Project::new();
    p.write("package.json", r#"{"devDependencies": {"vitest": "2"}}"#);
    p.write("pnpm-lock.yaml", "");
    assert_eq!(
        line(
            &p,
            "typescriptreact",
            "src/App.test.tsx",
            RunnableKind::Test,
            "renders"
        )
        .as_deref(),
        Some("pnpm exec vitest run src/App.test.tsx -t renders")
    );

    let plain = Project::new();
    plain.write("package.json", "{}");
    assert_eq!(
        line(
            &plain,
            "javascript",
            "test/a.test.mjs",
            RunnableKind::Test,
            "works"
        )
        .as_deref(),
        Some("node --test --test-name-pattern works test/a.test.mjs")
    );
    assert_eq!(
        line(&plain, "javascript", "index.js", RunnableKind::Main, "").as_deref(),
        Some("node index.js")
    );
}

#[test]
fn js_typescript_main_and_bun() {
    let p = Project::new();
    p.write("package.json", r#"{"devDependencies": {"tsx": "4"}}"#);
    assert_eq!(
        line(&p, "typescript", "src/main.ts", RunnableKind::Main, "").as_deref(),
        Some("npx --no-install tsx src/main.ts")
    );
    let bun = Project::new();
    bun.write("package.json", "{}");
    bun.write("bun.lock", "");
    assert_eq!(
        line(&bun, "typescript", "a.test.ts", RunnableKind::Test, "x").as_deref(),
        Some("bun test a.test.ts -t x")
    );
    assert_eq!(
        line(&bun, "typescript", "main.ts", RunnableKind::Main, "").as_deref(),
        Some("bun main.ts")
    );
}

#[test]
fn unsupported_language() {
    let p = Project::new();
    assert_eq!(line(&p, "cobol", "a.cob", RunnableKind::Main, ""), None);
}
