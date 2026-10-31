//! Turning a launch configuration into a ready [`DebugConfig`]: finding an
//! installed adapter, building the program (Cargo, or a configured build
//! command) and filling the launch arguments. Blocking: runs on the
//! background executor. Build output is streamed through `log`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arcode_config::DebugSettings;
use arcode_dap::{
    AdapterChoice, AdapterTransport, DebugConfig, KnownAdapter, LaunchOptions, LaunchVars,
    detect_adapter, locate_adapter, missing_adapter_message, spec_with_executable,
};
use arcode_project::{DebugTarget, cargo_message_text, executables_from_cargo_json};
use serde_json::json;

use super::launch::LaunchKind;

/// Everything [`prepare`] needs besides the configuration.
#[derive(Clone)]
pub struct PrepareInput {
    /// The project folder.
    pub root: PathBuf,
    /// The active file (for `${file}`).
    pub file: Option<PathBuf>,
    /// `[debug]` settings.
    pub settings: DebugSettings,
}

/// Lets the UI stop a running build.
#[derive(Clone, Default)]
pub struct Cancel {
    cancelled: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Cancel {
    /// Stops the build (kills the running process).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().ok().as_mut().and_then(|c| c.as_mut()) {
            // Also stops what the build started (rustc, a script's tools).
            arcode_project::proc_group::kill_group(child);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A started-able session: the config and the adapter's display name.
pub struct Prepared {
    /// The session config.
    pub config: DebugConfig,
}

type Log<'a> = &'a (dyn Fn(String) + Send + Sync);

/// Resolves the adapter, builds if needed and returns the config.
pub fn prepare(
    kind: &LaunchKind,
    input: &PrepareInput,
    cancel: &Cancel,
    log: Log<'_>,
) -> Result<Prepared, String> {
    let vars = LaunchVars {
        workspace_folder: input.root.clone(),
        file: input.file.clone(),
    };
    let mut config = match kind {
        LaunchKind::File(entry) => {
            let (spec, known) = match &entry.adapter {
                AdapterChoice::Known(adapter) => {
                    let path = configured(&input.settings, *adapter);
                    let found = locate_adapter(*adapter, path.as_deref()).ok_or_else(|| {
                        format!(
                            "{} was not found. Install {}. Nothing is downloaded automatically.",
                            adapter.name(),
                            adapter.install_help()
                        )
                    })?;
                    (spec_with_executable(*adapter, &found), Some(*adapter))
                }
                AdapterChoice::Language(language) => {
                    let (adapter, spec) = find(language, &input.settings)?;
                    (spec, Some(adapter))
                }
                AdapterChoice::Custom(spec) => {
                    spec.resolve_executable().map_err(|e| e.to_string())?;
                    ((**spec).clone(), None)
                }
            };
            if let Some(command) = &entry.build {
                run_shell(command, &entry.cwd(&vars), cancel, log)?;
            }
            let program = match &entry.cargo {
                Some(args) => {
                    let mut args = args.clone();
                    args.push("--message-format=json".into());
                    Some(cargo_build(&args, &input.root, cancel, log)?)
                }
                None => None,
            };
            entry.to_config(spec, known, &vars, program)
        }
        LaunchKind::Target(target) => target_config(target, input, cancel, log)?,
    };
    // The breakpoints are filled in when the session starts (they may
    // change while this builds).
    if let Some(var) = utf8_locale_fix(&config.adapter.name, &|k| std::env::var(k).ok()) {
        add_adapter_env(&mut config, var);
    }
    Ok(Prepared { config })
}

/// GDB's DAP never starts a program whose arguments are not ASCII when it
/// runs without a UTF-8 locale (it cannot encode them): give it one when
/// the environment has none. Returns the variable to set.
fn utf8_locale_fix(
    adapter: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<(String, String)> {
    if adapter != KnownAdapter::Gdb.name() || cfg!(windows) {
        return None;
    }
    let get = |k: &str| env(k).filter(|v| !v.is_empty());
    // The first set variable decides, as in setlocale(3).
    let locale = get("LC_ALL")
        .or_else(|| get("LC_CTYPE"))
        .or_else(|| get("LANG"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if locale.contains("utf-8") || locale.contains("utf8") {
        return None;
    }
    // LC_ALL overrides LC_CTYPE, so a non-UTF-8 LC_ALL is replaced.
    let key = if get("LC_ALL").is_some() {
        "LC_ALL"
    } else {
        "LC_CTYPE"
    };
    let value = if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    };
    Some((key.into(), value.into()))
}

fn add_adapter_env(config: &mut DebugConfig, (key, value): (String, String)) {
    let env = match &mut config.adapter.transport {
        AdapterTransport::Stdio { env, .. } => env,
        AdapterTransport::Tcp {
            command: Some(c), ..
        } => &mut c.env,
        AdapterTransport::Tcp { command: None, .. } => return,
    };
    env.retain(|(k, _)| *k != key);
    env.push((key, value));
}

fn configured(settings: &DebugSettings, adapter: KnownAdapter) -> Option<PathBuf> {
    settings.adapter_path(adapter.name()).map(PathBuf::from)
}

fn find(
    language: &str,
    settings: &DebugSettings,
) -> Result<(KnownAdapter, arcode_dap::AdapterSpec), String> {
    detect_adapter(language, settings.adapter_for(language), &|a| {
        configured(settings, a)
    })
    .map_err(|_| missing_adapter_message(language))
}

fn target_config(
    target: &DebugTarget,
    input: &PrepareInput,
    cancel: &Cancel,
    log: Log<'_>,
) -> Result<DebugConfig, String> {
    let (adapter, spec) = find(target.language, &input.settings)?;
    let program = match &target.build {
        Some(build) => cargo_build(&build.args, &build.cwd, cancel, log)?,
        None => target.program.clone().unwrap_or_default(),
    };
    let options = LaunchOptions {
        program,
        args: target.args.clone(),
        cwd: target.cwd.clone(),
        env: BTreeMap::new(),
        stop_on_entry: false,
    };
    let mut arguments = adapter.launch_arguments(&options);
    if let Some(module) = &target.module {
        arguments.as_object_mut().map(|m| m.remove("program"));
        arguments["module"] = json!(module);
    }
    if let Some(mode) = target.go_mode {
        arguments["mode"] = json!(mode);
    }
    Ok(DebugConfig::new(
        spec,
        arcode_dap::DebugRequest::Launch,
        arguments,
    ))
}

/// Runs `cargo <args>` (which must ask for JSON messages) and returns the
/// executable it built.
fn cargo_build(
    args: &[String],
    cwd: &Path,
    cancel: &Cancel,
    log: Log<'_>,
) -> Result<PathBuf, String> {
    log(format!("> cargo {}", args.join(" ")));
    let mut command = Command::new("cargo");
    command.args(args).current_dir(cwd);
    let stdout = run_streaming(command, cancel, log, true)?;
    let exes = executables_from_cargo_json(&stdout);
    let pick = exes
        .iter()
        .rev()
        .find(|e| e.kinds.iter().any(|k| k == "bin" || k == "example") && !e.test)
        .or_else(|| exes.first())
        .ok_or("cargo built no executable to debug")?;
    if exes.len() > 1 {
        log(format!(
            "Several executables were built; debugging {}",
            pick.path.display()
        ));
    }
    Ok(pick.path.clone())
}

/// Runs a configured build command line in the platform shell.
fn run_shell(command_line: &str, cwd: &Path, cancel: &Cancel, log: Log<'_>) -> Result<(), String> {
    log(format!("> {command_line}"));
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command_line]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command_line]);
        c
    };
    command.current_dir(cwd);
    run_streaming(command, cancel, log, false).map(|_| ())
}

/// Runs `command`, logging stderr (and stdout unless `json`), and returns
/// stdout. Fails on a non-zero exit or when cancelled.
fn run_streaming(
    mut command: Command,
    cancel: &Cancel,
    log: Log<'_>,
    json: bool,
) -> Result<String, String> {
    arcode_project::proc_group::own_group(&mut command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start the build: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut slot) = cancel.child.lock() {
        *slot = Some(child);
    }
    if cancel.is_cancelled() {
        cancel.cancel();
    }
    let mut out = String::new();
    std::thread::scope(|scope| {
        if let Some(stderr) = stderr {
            scope.spawn(|| {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log(line);
                }
            });
        }
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if json {
                    if let Some(text) = cargo_message_text(&line) {
                        log(text);
                    }
                } else {
                    log(line.clone());
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
    });
    let status = cancel
        .child
        .lock()
        .ok()
        .and_then(|mut c| c.take())
        .map(|mut c| c.wait());
    if cancel.is_cancelled() {
        return Err("Cancelled".into());
    }
    match status {
        Some(Ok(status)) if status.success() => Ok(out),
        Some(Ok(status)) => Err(format!("The build failed ({status})")),
        Some(Err(e)) => Err(format!("The build failed: {e}")),
        None => Err("The build was stopped".into()),
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
