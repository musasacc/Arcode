//! Sessions against real debug adapters. Ignored by default (they need the
//! adapter and a compiler installed); each skips itself if a tool is
//! missing. Run with `cargo test -p arcode-dap --test real_adapters -- --ignored`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use arcode_dap::*;
use common::{collect_until, wait_event};

const C_PROGRAM: &str = r#"#include <stdio.h>
int add(int a, int b) { int sum = a + b; return sum; }
int main(void) {
    int x = 40;
    int y = add(x, 2);
    printf("y=%d\n", y);
    return 3;
}
"#;

const PY_PROGRAM: &str = r#"def add(a, b):
    total = a + b
    return total

x = 40
y = add(x, 2)
print(f"y={y}")
"#;

fn skip(why: &str) {
    eprintln!("skipped: {why}");
}

fn is_stopped(e: &DebugEvent) -> bool {
    matches!(e, DebugEvent::StateChanged(SessionState::Stopped { .. }))
}

/// Compiles the C program with debug info; `None` if no compiler.
fn compile_c(dir: &Path) -> Option<PathBuf> {
    let source = dir.join("main.c");
    std::fs::write(&source, C_PROGRAM).ok()?;
    let out = dir.join(if cfg!(windows) { "main.exe" } else { "main" });
    for cc in ["cc", "gcc", "clang"] {
        if find_executable(cc, &[]).is_none() {
            continue;
        }
        let status = Command::new(cc)
            .args(["-g", "-O0", "-o"])
            .arg(&out)
            .arg(&source)
            .status();
        if status.map(|s| s.success()).unwrap_or(false) {
            return Some(out);
        }
    }
    None
}

fn gdb_has_dap() -> bool {
    let Ok(out) = Command::new("gdb").arg("--version").output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let major = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().last())
        .and_then(|v| v.split('.').next())
        .and_then(|m| m.parse::<u32>().ok());
    major.is_some_and(|m| m >= 14)
}

/// Breakpoint in `main`, inspect, step into `add`, continue to exit.
async fn native_scenario(adapter: KnownAdapter) {
    if !adapter.is_installed() || (adapter == KnownAdapter::Gdb && !gdb_has_dap()) {
        return skip(&format!("{} not installed", adapter.name()));
    }
    let dir = tempfile::tempdir().unwrap();
    let Some(program) = compile_c(dir.path()) else {
        return skip("no C compiler");
    };
    let options = LaunchOptions {
        program,
        cwd: dir.path().to_path_buf(),
        ..Default::default()
    };
    let mut config = adapter.launch_config(&options);
    let source = dir.path().join("main.c");
    config
        .breakpoints
        .insert(source.clone(), vec![SourceBreakpoint::at_line(5)]);
    let (session, rx) = DebugSession::start(config);
    let events = collect_until(&rx, |e| {
        is_stopped(e) || matches!(e, DebugEvent::AdapterFailed(_))
    })
    .await;
    if let Some(DebugEvent::AdapterFailed(m)) = events.last() {
        panic!("{} failed: {m}\n{events:#?}", adapter.name());
    }
    let set = events.iter().find_map(|e| match e {
        DebugEvent::BreakpointsSet { breakpoints, .. } => Some(breakpoints.clone()),
        _ => None,
    });
    assert_eq!(set.map(|b| b.len()), Some(1), "{events:#?}");
    let SessionState::Stopped { thread_id, reason } = session.state() else {
        unreachable!()
    };
    assert_eq!(reason, "breakpoint");
    let thread = match thread_id {
        Some(t) => t,
        None => session.threads().await.unwrap()[0].id,
    };
    let frames = session.stack_trace(thread).await.unwrap();
    assert_eq!(frames[0].line, 5, "{frames:#?}");
    assert!(frames[0].name.contains("main"), "{frames:#?}");
    let scopes = session.scopes(frames[0].id).await.unwrap();
    let mut found_x = false;
    for scope in scopes.iter().filter(|s| !s.expensive) {
        let vars = session.variables(scope.variables_reference).await.unwrap();
        found_x |= vars.iter().any(|v| v.name == "x" && v.value.contains("40"));
    }
    assert!(found_x, "x = 40 in some scope: {scopes:#?}");
    let eval = session
        .evaluate("x + 2", Some(frames[0].id), EvaluateContext::Watch)
        .await
        .unwrap();
    assert!(eval.result.contains("42"), "{eval:?}");

    session.step_in(thread).await.unwrap();
    wait_event(&rx, is_stopped).await;
    let frames = session.stack_trace(thread).await.unwrap();
    assert!(frames[0].name.contains("add"), "{frames:#?}");

    session.continue_(thread).await.unwrap();
    let tail = collect_until(&rx, |e| {
        matches!(
            e,
            DebugEvent::AdapterExited(_) | DebugEvent::AdapterFailed(_)
        )
    })
    .await;
    assert!(
        tail.iter()
            .any(|e| matches!(e, DebugEvent::Exited(x) if x.exit_code == 3)),
        "{tail:#?}"
    );
    assert_eq!(session.state(), SessionState::Terminated);
}

async fn with_timeout(fut: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(120), fut)
        .await
        .expect("real adapter scenario timed out");
}

#[tokio::test]
#[ignore = "needs gdb >= 14 and a C compiler"]
async fn gdb_session() {
    with_timeout(native_scenario(KnownAdapter::Gdb)).await;
}

#[tokio::test]
#[ignore = "needs lldb-dap and a C compiler"]
async fn lldb_dap_session() {
    with_timeout(native_scenario(KnownAdapter::LldbDap)).await;
}

#[tokio::test]
#[ignore = "needs codelldb and a C compiler"]
async fn codelldb_session() {
    with_timeout(native_scenario(KnownAdapter::CodeLldb)).await;
}

fn debugpy_installed() -> bool {
    let python = KnownAdapter::Debugpy.executable();
    Command::new(python)
        .args(["-c", "import debugpy"])
        .stdin(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore = "needs python with debugpy"]
async fn debugpy_session_runs_in_the_client_terminal() {
    if !debugpy_installed() {
        return skip("debugpy not installed");
    }
    with_timeout(async {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("main.py");
        std::fs::write(&script, PY_PROGRAM).unwrap();
        let options = LaunchOptions {
            program: script.clone(),
            cwd: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut config = KnownAdapter::Debugpy.launch_config(&options);
        config
            .breakpoints
            .insert(script.clone(), vec![SourceBreakpoint::at_line(2)]);
        let (session, rx) = DebugSession::start(config);
        // `console: integratedTerminal`: the "terminal" here is a plain child.
        let event = wait_event(&rx, |e| {
            matches!(
                e,
                DebugEvent::RunInTerminal(_) | DebugEvent::AdapterFailed(_)
            )
        })
        .await;
        let DebugEvent::RunInTerminal(request) = event else {
            panic!("{event:?}");
        };
        let args = &request.arguments.args;
        let mut cmd = Command::new(&args[0]);
        cmd.args(&args[1..]).current_dir(&request.arguments.cwd);
        for (k, v) in request.arguments.env.clone().unwrap_or_default() {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        let mut child = cmd.stdin(std::process::Stdio::null()).spawn().unwrap();
        request.respond(Some(child.id()), None).unwrap();

        wait_event(&rx, is_stopped).await;
        let thread = session.threads().await.unwrap()[0].id;
        let frames = session.stack_trace(thread).await.unwrap();
        assert_eq!(frames[0].line, 2);
        assert_eq!(frames[0].name, "add");
        let scopes = session.scopes(frames[0].id).await.unwrap();
        let vars = session
            .variables(scopes[0].variables_reference)
            .await
            .unwrap();
        assert!(
            vars.iter().any(|v| v.name == "a" && v.value == "40"),
            "{vars:#?}"
        );
        session.continue_(thread).await.unwrap();
        wait_event(&rx, |e| matches!(e, DebugEvent::Terminated(_))).await;
        let _ = child.wait();
    })
    .await;
}

#[tokio::test]
#[ignore = "needs gdb >= 14 and a C compiler"]
async fn gdb_restart_and_stop() {
    if !KnownAdapter::Gdb.is_installed() || !gdb_has_dap() {
        return skip("gdb with DAP not installed");
    }
    let dir = tempfile::tempdir().unwrap();
    let Some(program) = compile_c(dir.path()) else {
        return skip("no C compiler");
    };
    with_timeout(async {
        let options = LaunchOptions {
            program,
            cwd: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut config = KnownAdapter::Gdb.launch_config(&options);
        config.breakpoints.insert(
            dir.path().join("main.c"),
            vec![SourceBreakpoint::at_line(2)],
        );
        let (session, rx) = DebugSession::start(config);
        wait_event(&rx, is_stopped).await;
        // gdb has no `restart`: the session respawns the adapter.
        assert!(!session.capabilities().supports_restart_request);
        session.restart().await.unwrap();
        wait_event(&rx, is_stopped).await;
        let thread = session.threads().await.unwrap()[0].id;
        let frames = session.stack_trace(thread).await.unwrap();
        assert!(frames[0].name.contains("add"), "{frames:#?}");
        session.stop().await.unwrap();
        assert_eq!(session.state(), SessionState::Terminated);
        wait_event(&rx, |e| matches!(e, DebugEvent::AdapterExited(_))).await;
    })
    .await;
}
