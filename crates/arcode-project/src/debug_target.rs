//! What to debug for a run command: the build step (Cargo), the program,
//! its arguments, and Cargo's JSON output that names the built executable.
//!
//! Pure logic: nothing is spawned here. The UI runs [`DebugTarget::build`]
//! (off the UI thread), reads the executable with
//! [`executables_from_cargo_json`] and starts a debug adapter.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::command::{RunCommand, RunSource};

/// How to debug one [`RunCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugTarget {
    /// LSP language id that picks the debug adapter (`rust`, `python`,
    /// `go`, `javascript`).
    pub language: &'static str,
    /// A command that builds the program first, printing Cargo's JSON
    /// messages (`--message-format=json`); its executable is the program.
    pub build: Option<RunCommand>,
    /// The program (a script, a Go package folder); `None` when `build`
    /// produces it.
    pub program: Option<PathBuf>,
    /// A Python module to run instead of a program (`pytest`).
    pub module: Option<String>,
    /// Program arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Delve's mode: `debug` or `test`.
    pub go_mode: Option<&'static str>,
}

/// Derives the debug target of a run, test or bench command. `None` when
/// the command cannot be debugged (a Node script, `make`, a shell recipe).
pub fn debug_target(command: &RunCommand) -> Option<DebugTarget> {
    let base = DebugTarget {
        language: "rust",
        build: None,
        program: None,
        module: None,
        args: Vec::new(),
        cwd: command.cwd.clone(),
        go_mode: None,
    };
    match command.source {
        RunSource::Cargo => cargo_target(command, base),
        RunSource::Python => python_target(command, base),
        RunSource::Go => go_target(command, base),
        RunSource::Node | RunSource::Make | RunSource::Just => None,
    }
}

fn cargo_target(command: &RunCommand, base: DebugTarget) -> Option<DebugTarget> {
    let split = command
        .args
        .iter()
        .position(|a| a == "--")
        .unwrap_or(command.args.len());
    let (cargo_args, rest) = command.args.split_at(split);
    let program_args: Vec<String> = rest.iter().skip(1).cloned().collect();
    let (sub, flags) = cargo_args.split_first()?;
    let mut build_args: Vec<String> = match sub.as_str() {
        "run" => vec!["build".into()],
        "test" | "bench" => vec![sub.clone(), "--no-run".into()],
        "build" => vec!["build".into()],
        _ => return None,
    };
    build_args.extend(
        flags
            .iter()
            .filter(|f| !f.starts_with("--message-format"))
            .cloned(),
    );
    build_args.push("--message-format=json".into());
    let build = RunCommand::new(
        command.program.clone(),
        build_args,
        &command.cwd,
        crate::RunKind::Build,
        RunSource::Cargo,
    );
    Some(DebugTarget {
        build: Some(build),
        args: program_args,
        ..base
    })
}

fn python_target(command: &RunCommand, base: DebugTarget) -> Option<DebugTarget> {
    let args = &command.args;
    if args.len() >= 2 && args[0] == "-m" {
        return Some(DebugTarget {
            language: "python",
            module: Some(args[1].clone()),
            args: args[2..].to_vec(),
            ..base
        });
    }
    let (script, rest) = args.split_first()?;
    Some(DebugTarget {
        language: "python",
        program: Some(absolute(&command.cwd, script)),
        args: rest.to_vec(),
        ..base
    })
}

fn go_target(command: &RunCommand, base: DebugTarget) -> Option<DebugTarget> {
    let (sub, rest) = command.args.split_first()?;
    let package = rest.last()?;
    let program = Some(absolute(&command.cwd, package));
    match sub.as_str() {
        "run" => Some(DebugTarget {
            language: "go",
            program,
            go_mode: Some("debug"),
            ..base
        }),
        "test" => {
            let mut args = Vec::new();
            let mut flags = rest[..rest.len() - 1].iter();
            while let Some(flag) = flags.next() {
                if let ("-run" | "-bench", Some(value)) = (flag.as_str(), flags.next()) {
                    args.push(format!("-test.{}", &flag[1..]));
                    args.push(value.clone());
                }
            }
            Some(DebugTarget {
                language: "go",
                program,
                args,
                go_mode: Some("test"),
                ..base
            })
        }
        _ => None,
    }
}

fn absolute(cwd: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// An executable Cargo built, from its JSON messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoExecutable {
    /// Target name (`app`, `integration`).
    pub target: String,
    /// Target kinds (`bin`, `lib`, `test`, `example`, `bench`).
    pub kinds: Vec<String>,
    /// Whether it is a test harness build.
    pub test: bool,
    /// The executable.
    pub path: PathBuf,
}

/// The executables in `cargo ... --message-format=json` output (one JSON
/// object per line; other lines are ignored), in build order.
pub fn executables_from_cargo_json(stdout: &str) -> Vec<CargoExecutable> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if msg["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(path) = msg["executable"].as_str() else {
            continue;
        };
        let kinds = msg["target"]["kind"]
            .as_array()
            .map(|k| {
                k.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(CargoExecutable {
            target: msg["target"]["name"].as_str().unwrap_or_default().into(),
            kinds,
            test: msg["profile"]["test"].as_bool().unwrap_or(false),
            path: PathBuf::from(path),
        });
    }
    out
}

/// A human-readable line for a Cargo JSON message worth showing while
/// building (`Compiling app`, rendered compiler errors); `None` for others.
pub fn cargo_message_text(line: &str) -> Option<String> {
    let msg: Value = serde_json::from_str(line).ok()?;
    match msg["reason"].as_str()? {
        "compiler-message" => msg["message"]["rendered"]
            .as_str()
            .map(|s| s.trim_end().to_string()),
        "compiler-artifact" if msg["fresh"] == false => msg["target"]["name"]
            .as_str()
            .map(|n| format!("   Compiling {n}")),
        "build-finished" => Some(if msg["success"] == true {
            "    Finished".to_string()
        } else {
            "Build failed".to_string()
        }),
        _ => None,
    }
}

#[cfg(test)]
#[path = "debug_target_tests.rs"]
mod tests;
