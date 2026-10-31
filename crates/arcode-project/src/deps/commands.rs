//! The manager commands behind the Packages view's actions: add, remove,
//! update one package, update all. Arguments are separate entries (never
//! a shell line); names and versions are validated first.

use std::path::{Path, PathBuf};

use super::validate::{validate_package_name, validate_version};
use super::{DepKind, Manager, ProjectManifest};
use crate::command::command_line;

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageAction {
    /// Add a package, optionally at a version, optionally as a
    /// development dependency.
    Add {
        /// Package name.
        name: String,
        /// Version or range (`None`: the latest).
        version: Option<String>,
        /// Development dependency.
        dev: bool,
    },
    /// Remove a declared dependency.
    Remove {
        /// The name as written in the manifest.
        name: String,
        /// Its kind (some managers need `--dev`).
        kind: DepKind,
        /// Its group (Poetry and uv groups, Python extras).
        group: Option<String>,
    },
    /// Update one package (within the manifest's requirement).
    Update {
        /// Package name.
        name: String,
    },
    /// Update every package.
    UpdateAll,
}

/// A command to run in a terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCommand {
    /// Program name (resolved via `PATH`) or absolute path.
    pub program: String,
    /// Arguments, unquoted, one per entry.
    pub args: Vec<String>,
    /// Working directory: the manifest's folder.
    pub cwd: PathBuf,
    /// The command line for display (program relative to `cwd`).
    pub display: String,
}

impl PackageCommand {
    pub(crate) fn new(program: String, args: Vec<String>, cwd: &Path, shown: Option<&str>) -> Self {
        let display = command_line(shown.unwrap_or(&program), &args);
        Self {
            program,
            args,
            cwd: cwd.to_path_buf(),
            display,
        }
    }
}

/// The program for `manager` on the target platform: npm, pnpm and Yarn
/// install `.cmd` shims on Windows, Composer and Bundler `.bat` files,
/// which a process cannot be started from without the extension.
pub(crate) fn program_name(manager: Manager, windows: bool) -> String {
    let base = manager.program();
    match manager {
        Manager::Npm | Manager::Pnpm | Manager::Yarn if windows => format!("{base}.cmd"),
        Manager::Composer | Manager::Bundler if windows => format!("{base}.bat"),
        _ => base.to_string(),
    }
}

/// Whether the program is a batch file `cmd.exe` interprets.
pub(crate) fn is_cmd_shim(manager: Manager, windows: bool) -> bool {
    windows
        && matches!(
            manager,
            Manager::Npm | Manager::Pnpm | Manager::Yarn | Manager::Composer | Manager::Bundler
        )
}

/// The interpreter pip runs with, and how to show it.
pub(crate) fn python_program(m: &ProjectManifest, windows: bool) -> (String, Option<String>) {
    match &m.python {
        Some(python) => {
            let shown = crate::fsutil::rel_slash(python, &m.dir);
            (python.to_string_lossy().into_owned(), Some(shown))
        }
        None => (
            (if windows { "python" } else { "python3" }).to_string(),
            None,
        ),
    }
}

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| (*a).to_string()).collect()
}

/// `name` with a version in the manager's syntax.
fn with_version(manager: Manager, name: &str, version: Option<&str>) -> String {
    let Some(v) = version else {
        return name.to_string();
    };
    match manager {
        Manager::Uv | Manager::Pipenv | Manager::Pip => {
            if v.starts_with(|c: char| c.is_ascii_digit()) {
                format!("{name}=={v}")
            } else {
                format!("{name}{v}")
            }
        }
        Manager::Composer => format!("{name}:{v}"),
        _ => format!("{name}@{v}"),
    }
}

/// The command for `action` on manifest `m`. `windows` selects the
/// Windows program names. Fails on an invalid name or version, or an
/// action the manager has no command for.
pub fn action_command(
    m: &ProjectManifest,
    action: &PackageAction,
    windows: bool,
) -> Result<PackageCommand, String> {
    let manager = m.manager;
    let shim = is_cmd_shim(manager, windows);
    let (name, version) = match action {
        PackageAction::Add { name, version, .. } => (Some(name), version.as_deref()),
        PackageAction::Remove { name, .. } | PackageAction::Update { name } => (Some(name), None),
        PackageAction::UpdateAll => (None, None),
    };
    if let Some(name) = name {
        validate_package_name(name)?;
    }
    if let Some(version) = version {
        validate_version(version, shim)?;
    }
    if let PackageAction::Remove { group: Some(g), .. } = action {
        validate_package_name(g).map_err(|_| format!("Unexpected group name {g:?}"))?;
    }
    let args = match manager {
        Manager::Pip => return pip_command(m, action, windows),
        Manager::Cargo => cargo_args(action),
        Manager::Npm => npm_args(action),
        Manager::Pnpm | Manager::Bun => pnpm_bun_args(manager, action),
        Manager::Yarn => yarn_args(action, m.yarn_berry),
        Manager::Uv => uv_args(action),
        Manager::Poetry => poetry_args(action),
        Manager::Pipenv => pipenv_args(action),
        Manager::Go => go_args(action),
        Manager::Composer => composer_args(action),
        Manager::Bundler => bundler_args(action),
    };
    Ok(PackageCommand::new(
        program_name(manager, windows),
        args,
        &m.dir,
        None,
    ))
}

fn cargo_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "add",
                &with_version(Manager::Cargo, name, version.as_deref()),
            ]);
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Remove { name, kind, .. } => {
            let mut a = s(&["remove", name]);
            match kind {
                DepKind::Dev => a.push("--dev".into()),
                DepKind::Build => a.push("--build".into()),
                _ => {}
            }
            a
        }
        PackageAction::Update { name } => s(&["update", "-p", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

fn npm_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "install",
                &with_version(Manager::Npm, name, version.as_deref()),
            ]);
            if *dev {
                a.push("--save-dev".into());
            }
            a
        }
        PackageAction::Remove { name, .. } => s(&["uninstall", name]),
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

fn pnpm_bun_args(manager: Manager, action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&["add", &with_version(manager, name, version.as_deref())]);
            if *dev {
                a.push(
                    if manager == Manager::Pnpm {
                        "--save-dev"
                    } else {
                        "--dev"
                    }
                    .into(),
                );
            }
            a
        }
        PackageAction::Remove { name, .. } => s(&["remove", name]),
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

fn yarn_args(action: &PackageAction, berry: bool) -> Vec<String> {
    let upgrade = if berry { "up" } else { "upgrade" };
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "add",
                &with_version(Manager::Yarn, name, version.as_deref()),
            ]);
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Remove { name, .. } => s(&["remove", name]),
        PackageAction::Update { name } => s(&[upgrade, name]),
        // Berry's `up` needs a pattern; `*` matches every dependency.
        PackageAction::UpdateAll if berry => s(&["up", "*"]),
        PackageAction::UpdateAll => s(&["upgrade"]),
    }
}

/// `--dev` for the dev group, `--group g` for other groups, `--optional
/// extra` for extras.
fn python_group_args(kind: DepKind, group: Option<&str>, poetry: bool) -> Vec<String> {
    match (kind, group) {
        (DepKind::Optional, Some(extra)) if !poetry => s(&["--optional", extra]),
        (DepKind::Dev, Some(g)) if g != "dev" || poetry => s(&["--group", g]),
        (DepKind::Dev, _) => s(&["--dev"]),
        _ => Vec::new(),
    }
}

fn uv_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&["add", &with_version(Manager::Uv, name, version.as_deref())]);
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Remove { name, kind, group } => {
            let mut a = s(&["remove", name]);
            a.extend(python_group_args(*kind, group.as_deref(), false));
            a
        }
        PackageAction::Update { name } => s(&["lock", "--upgrade-package", name]),
        PackageAction::UpdateAll => s(&["lock", "--upgrade"]),
    }
}

fn poetry_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "add",
                &with_version(Manager::Poetry, name, version.as_deref()),
            ]);
            if *dev {
                a.extend(s(&["--group", "dev"]));
            }
            a
        }
        PackageAction::Remove { name, kind, group } => {
            let mut a = s(&["remove", name]);
            a.extend(python_group_args(*kind, group.as_deref(), true));
            a
        }
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

fn pipenv_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "install",
                &with_version(Manager::Pipenv, name, version.as_deref()),
            ]);
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Remove { name, .. } => s(&["uninstall", name]),
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

/// pip runs as `<python> -m pip ...` in the project's virtualenv when
/// there is one.
fn pip_command(
    m: &ProjectManifest,
    action: &PackageAction,
    windows: bool,
) -> Result<PackageCommand, String> {
    let mut args = s(&["-m", "pip"]);
    match action {
        PackageAction::Add { name, version, .. } => {
            args.extend(s(&[
                "install",
                &with_version(Manager::Pip, name, version.as_deref()),
            ]));
        }
        PackageAction::Remove { name, .. } => args.extend(s(&["uninstall", "-y", name])),
        PackageAction::Update { name } => args.extend(s(&["install", "-U", name])),
        PackageAction::UpdateAll => {
            let file = m.file_name();
            if file == "pyproject.toml" {
                args.extend(s(&["install", "-U", "-e", "."]));
            } else {
                args.extend(s(&["install", "-U", "-r", &file]));
            }
        }
    }
    let (program, shown) = python_program(m, windows);
    Ok(PackageCommand::new(program, args, &m.dir, shown.as_deref()))
}

fn go_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, .. } => {
            s(&["get", &with_version(Manager::Go, name, version.as_deref())])
        }
        PackageAction::Remove { name, .. } => s(&["get", &format!("{name}@none")]),
        PackageAction::Update { name } => s(&["get", "-u", name]),
        PackageAction::UpdateAll => s(&["get", "-u", "./..."]),
    }
}

fn composer_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&[
                "require",
                &with_version(Manager::Composer, name, version.as_deref()),
            ]);
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Remove { name, kind, .. } => {
            let mut a = s(&["remove", name]);
            if *kind == DepKind::Dev {
                a.push("--dev".into());
            }
            a
        }
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}

fn bundler_args(action: &PackageAction) -> Vec<String> {
    match action {
        PackageAction::Add { name, version, dev } => {
            let mut a = s(&["add", name]);
            if let Some(v) = version {
                a.extend(s(&["--version", v]));
            }
            if *dev {
                a.extend(s(&["--group", "development"]));
            }
            a
        }
        PackageAction::Remove { name, .. } => s(&["remove", name]),
        PackageAction::Update { name } => s(&["update", name]),
        PackageAction::UpdateAll => s(&["update"]),
    }
}
