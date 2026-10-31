//! Python manifests: uv, Poetry, Pipenv and pip with a virtualenv.

use super::*;
use crate::toolchain;

#[test]
fn uv_project_with_groups_and_lock() {
    let (_dir, root) = project();
    write(&root, "pyproject.toml", fx!("pyproject_uv.toml"));
    write(&root, "uv.lock", fx!("uv.lock"));
    let found = scan_manifests(&root);
    assert_eq!(listing(&found, &root), ["uv pyproject.toml"]);
    let m = &found[0];
    assert_eq!(m.name.as_deref(), Some("data-tool"));
    let summary: Vec<_> = m
        .deps
        .iter()
        .map(|d| {
            format!(
                "{} {} {:?} {} {} {}",
                d.name,
                d.requested,
                d.kind,
                d.group.as_deref().unwrap_or("-"),
                d.locked.as_deref().unwrap_or("-"),
                d.line.map_or(-1, |l| l as i64)
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            "httpx >=0.26 Normal - 0.26.0 5",
            "Pydantic ~=2.5 Normal - 2.5.3 6",
            "rich  Normal - - 7",
            "mylib  Normal - - 8",
            "matplotlib >=3.8 Optional plot - 12",
            "pytest >=7.4 Dev dev 7.4.4 16",
            "ruff ==0.1.14 Dev lint 0.1.14 19",
            "mypy >=1.8 Dev dev - 22",
        ]
    );
    assert!(matches!(dep(m, "mylib").source, DepSource::Git(_)));
}

#[test]
fn poetry_project() {
    let (_dir, root) = project();
    write(&root, "svc/pyproject.toml", fx!("pyproject_poetry.toml"));
    write(&root, "svc/poetry.lock", fx!("poetry.lock"));
    let found = scan_manifests(&root);
    let m = &found[0];
    assert_eq!(m.manager, Manager::Poetry);
    assert!(m.deps.iter().all(|d| d.name != "python"));
    let fastapi = dep(m, "fastapi");
    assert_eq!(
        (fastapi.locked.as_deref(), fastapi.line),
        (Some("0.109.0"), Some(8))
    );
    let uvicorn = dep(m, "uvicorn");
    assert_eq!(
        (uvicorn.requested.as_str(), uvicorn.locked.as_deref()),
        ("^0.27.0", Some("0.27.0.post1"))
    );
    assert_eq!(dep(m, "sqlalchemy").kind, DepKind::Optional);
    assert_eq!(
        dep(m, "internal").source,
        DepSource::Path("../internal".into())
    );
    let pytest = dep(m, "pytest");
    assert_eq!(
        (pytest.kind, pytest.group.as_deref(), pytest.line),
        (DepKind::Dev, Some("dev"), Some(14))
    );
    assert_eq!(dep(m, "mkdocs").group.as_deref(), Some("docs"));

    // Without a lockfile the `[tool.poetry]` table decides.
    std::fs::remove_file(root.join("svc/poetry.lock")).unwrap();
    assert_eq!(scan_manifests(&root)[0].manager, Manager::Poetry);
}

#[test]
fn pipenv_project() {
    let (_dir, root) = project();
    write(&root, "Pipfile", fx!("Pipfile"));
    write(&root, "Pipfile.lock", fx!("Pipfile.lock"));
    let found = scan_manifests(&root);
    let m = &found[0];
    assert_eq!(m.manager, Manager::Pipenv);
    let requests = dep(m, "requests");
    assert_eq!(
        (
            requests.requested.as_str(),
            requests.locked.as_deref(),
            requests.line
        ),
        ("*", Some("2.31.0"), Some(6))
    );
    assert_eq!(dep(m, "celery").requested, ">=5.3");
    let black = dep(m, "black");
    assert_eq!(
        (black.kind, black.locked.as_deref(), black.line),
        (DepKind::Dev, Some("23.12.1"), Some(11))
    );
}

#[test]
fn requirements_with_a_virtualenv() {
    let (_dir, root) = project();
    write(&root, "requirements.txt", fx!("requirements.txt"));
    write(&root, ".venv/pyvenv.cfg", "home = /usr/bin\n");
    let python = toolchain::venv_python(&root.join(".venv"));
    write(
        &root,
        python.strip_prefix(&root).unwrap().to_str().unwrap(),
        "",
    );
    std::fs::create_dir_all(root.join(".venv/lib/python3.12/site-packages/Flask-3.0.0.dist-info"))
        .unwrap();
    let found = scan_manifests(&root);
    assert_eq!(listing(&found, &root), ["pip requirements.txt"]);
    let m = &found[0];
    assert_eq!(m.python.as_deref(), Some(python.as_path()));
    let names: Vec<_> = m.deps.iter().map(|d| (d.name.as_str(), d.line)).collect();
    assert_eq!(
        names,
        [
            ("flask", Some(1)),
            ("gunicorn", Some(2)),
            ("SQLAlchemy", Some(3)),
            ("helpers", Some(6))
        ]
    );
    assert_eq!(dep(m, "flask").locked.as_deref(), Some("3.0.0"));
    assert_eq!(dep(m, "SQLAlchemy").requested, ">=2.0,<3");
    assert!(matches!(dep(m, "helpers").source, DepSource::Git(_)));
}

#[test]
fn pyproject_manager_without_lockfile() {
    let (_dir, root) = project();
    write(
        &root,
        "a/pyproject.toml",
        "[project]\nname = \"a\"\ndependencies = [\"x\"]\n\n[tool.uv]\n",
    );
    write(
        &root,
        "b/pyproject.toml",
        "[project]\nname = \"b\"\ndependencies = []\n",
    );
    let found = scan_manifests(&root);
    assert_eq!(
        listing(&found, &root),
        ["uv a/pyproject.toml", "pip b/pyproject.toml"]
    );
    assert_eq!(found[1].python, None);
}
