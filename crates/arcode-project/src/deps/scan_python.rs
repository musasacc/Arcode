//! Reading Python manifests (`pyproject.toml`, `Pipfile`,
//! `requirements*.txt`): the manager from the lockfile or tool tables,
//! versions from the lockfile or the project's virtualenv.

use std::path::{Path, PathBuf};

use super::python_locks::{self, Versions};
use super::scan::{file_name, lock_error, nearest, read_simple, read_text};
use super::{Manager, ProjectManifest, normalize_python, python_deps, requirements};
use crate::toolchain;

/// Python versions: the lockfile's, else the project virtualenv's.
fn apply_python_versions(m: &mut ProjectManifest, root: &Path, lock: Option<PathBuf>) {
    let mut versions = Versions::new();
    if let Some(lock) = &lock {
        let parsed = read_text(lock).and_then(|t| {
            if file_name(lock) == "Pipfile.lock" {
                python_locks::pipfile_lock(&t)
            } else {
                python_locks::toml_lock(&t)
            }
        });
        match parsed {
            Ok(v) => versions = v,
            Err(e) => lock_error(m, lock, e),
        }
    }
    let venv = toolchain::find_usable_python_venv(&m.dir, Some(root));
    if versions.is_empty()
        && let Some(venv) = &venv
    {
        versions = python_locks::venv_versions(&venv.dir);
    }
    m.python = venv.map(|v| v.python);
    m.lockfile = lock;
    for dep in &mut m.deps {
        dep.locked = versions.get(&normalize_python(&dep.name)).cloned();
    }
}

pub(super) fn read_pyproject(path: &Path, root: &Path) -> Option<ProjectManifest> {
    let mut flags = (false, false, true);
    let mut m = read_simple(path, Manager::Pip, |m, text| {
        let p = python_deps::parse_pyproject(text)?;
        flags = (p.poetry, p.uv, p.is_project);
        m.name = p.name;
        m.deps = p.deps;
        Ok(())
    });
    let (poetry, uv, is_project) = flags;
    if !is_project {
        return None;
    }
    let dir = m.dir.clone();
    let lock = nearest(&dir, root, "uv.lock").or_else(|| nearest(&dir, root, "poetry.lock"));
    m.manager = match lock.as_ref().map(|l| file_name(l)) {
        Some(l) if l == "uv.lock" => Manager::Uv,
        Some(_) => Manager::Poetry,
        None if poetry => Manager::Poetry,
        None if uv => Manager::Uv,
        None => Manager::Pip,
    };
    apply_python_versions(&mut m, root, lock);
    Some(m)
}

pub(super) fn read_pipfile(path: &Path, root: &Path) -> ProjectManifest {
    let mut m = read_simple(path, Manager::Pipenv, |m, text| {
        m.deps = python_deps::parse_pipfile(text)?;
        Ok(())
    });
    let lock = Some(m.dir.join("Pipfile.lock")).filter(|p| p.is_file());
    apply_python_versions(&mut m, root, lock);
    m
}

pub(super) fn read_requirements(path: &Path, root: &Path) -> ProjectManifest {
    let mut m = read_simple(path, Manager::Pip, |m, text| {
        m.deps = requirements::parse_requirements_file(text);
        Ok(())
    });
    apply_python_versions(&mut m, root, None);
    m
}
