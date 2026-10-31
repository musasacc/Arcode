//! The Python managers' commands (uv, Poetry, Pipenv, pip) for
//! [`super::action_command`].

use super::commands::{PackageAction, PackageCommand, python_program, s, with_version};
use super::{DepKind, Manager, ProjectManifest};

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

pub(super) fn uv_args(action: &PackageAction) -> Vec<String> {
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

pub(super) fn poetry_args(action: &PackageAction) -> Vec<String> {
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

pub(super) fn pipenv_args(action: &PackageAction) -> Vec<String> {
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
pub(super) fn pip_command(
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
