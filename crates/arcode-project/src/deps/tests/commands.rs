//! Exact argument lists of every manager's commands.

use super::*;

fn manifest(manager: Manager, file: &str) -> ProjectManifest {
    ProjectManifest::new(manager, PathBuf::from("/work/proj").join(file))
}

fn add(name: &str, version: Option<&str>, dev: bool) -> PackageAction {
    PackageAction::Add {
        name: name.into(),
        version: version.map(str::to_string),
        dev,
    }
}

fn remove(name: &str, kind: DepKind, group: Option<&str>) -> PackageAction {
    PackageAction::Remove {
        name: name.into(),
        kind,
        group: group.map(str::to_string),
    }
}

fn update(name: &str) -> PackageAction {
    PackageAction::Update { name: name.into() }
}

/// `program arg arg ...` for `action` (Linux names).
fn argv(m: &ProjectManifest, action: &PackageAction) -> String {
    argv_on(m, action, false)
}

fn argv_on(m: &ProjectManifest, action: &PackageAction, windows: bool) -> String {
    let cmd = action_command(m, action, windows).expect("command");
    assert_eq!(cmd.cwd, m.dir, "runs in the manifest's folder");
    let mut parts = vec![cmd.program];
    parts.extend(cmd.args);
    parts.join(" | ")
}

#[test]
fn cargo_commands() {
    let m = manifest(Manager::Cargo, "Cargo.toml");
    assert_eq!(argv(&m, &add("serde", None, false)), "cargo | add | serde");
    assert_eq!(
        argv(&m, &add("serde", Some("1.0.190"), true)),
        "cargo | add | serde@1.0.190 | --dev"
    );
    assert_eq!(
        argv(&m, &remove("rand08", DepKind::Normal, None)),
        "cargo | remove | rand08"
    );
    assert_eq!(
        argv(&m, &remove("tempfile", DepKind::Dev, None)),
        "cargo | remove | tempfile | --dev"
    );
    assert_eq!(
        argv(&m, &remove("cc", DepKind::Build, None)),
        "cargo | remove | cc | --build"
    );
    assert_eq!(argv(&m, &update("rand")), "cargo | update | -p | rand");
    assert_eq!(argv(&m, &PackageAction::UpdateAll), "cargo | update");
}

#[test]
fn node_commands_and_windows_shims() {
    let npm = manifest(Manager::Npm, "package.json");
    assert_eq!(
        argv(&npm, &add("@types/node", Some("^20"), true)),
        "npm | install | @types/node@^20 | --save-dev"
    );
    assert_eq!(
        argv(&npm, &remove("lodash", DepKind::Normal, None)),
        "npm | uninstall | lodash"
    );
    assert_eq!(argv(&npm, &update("lodash")), "npm | update | lodash");
    assert_eq!(argv(&npm, &PackageAction::UpdateAll), "npm | update");
    assert_eq!(
        argv_on(&npm, &add("react", None, false), true),
        "npm.cmd | install | react"
    );

    let pnpm = manifest(Manager::Pnpm, "package.json");
    assert_eq!(
        argv(&pnpm, &add("vite", None, true)),
        "pnpm | add | vite | --save-dev"
    );
    assert_eq!(
        argv(&pnpm, &remove("vite", DepKind::Dev, None)),
        "pnpm | remove | vite"
    );
    assert_eq!(argv(&pnpm, &update("vite")), "pnpm | update | vite");
    assert_eq!(
        argv_on(&pnpm, &PackageAction::UpdateAll, true),
        "pnpm.cmd | update"
    );

    let mut yarn = manifest(Manager::Yarn, "package.json");
    assert_eq!(
        argv(&yarn, &add("jest", Some("29"), true)),
        "yarn | add | jest@29 | --dev"
    );
    assert_eq!(
        argv(&yarn, &remove("jest", DepKind::Dev, None)),
        "yarn | remove | jest"
    );
    assert_eq!(argv(&yarn, &update("jest")), "yarn | upgrade | jest");
    assert_eq!(argv(&yarn, &PackageAction::UpdateAll), "yarn | upgrade");
    assert_eq!(
        argv_on(&yarn, &update("jest"), true),
        "yarn.cmd | upgrade | jest"
    );
    yarn.yarn_berry = true;
    assert_eq!(argv(&yarn, &update("jest")), "yarn | up | jest");
    assert_eq!(argv(&yarn, &PackageAction::UpdateAll), "yarn | up | *");

    let bun = manifest(Manager::Bun, "package.json");
    assert_eq!(
        argv(&bun, &add("hono", None, true)),
        "bun | add | hono | --dev"
    );
    assert_eq!(
        argv(&bun, &remove("hono", DepKind::Normal, None)),
        "bun | remove | hono"
    );
    assert_eq!(argv(&bun, &update("hono")), "bun | update | hono");
    assert_eq!(
        argv_on(&bun, &update("hono"), true),
        "bun | update | hono",
        "bun is an .exe"
    );
}

#[test]
fn python_commands() {
    let uv = manifest(Manager::Uv, "pyproject.toml");
    assert_eq!(
        argv(&uv, &add("httpx", Some("0.26.0"), false)),
        "uv | add | httpx==0.26.0"
    );
    assert_eq!(
        argv(&uv, &add("httpx", Some(">=0.26"), true)),
        "uv | add | httpx>=0.26 | --dev"
    );
    assert_eq!(
        argv(&uv, &remove("pytest", DepKind::Dev, Some("dev"))),
        "uv | remove | pytest | --dev"
    );
    assert_eq!(
        argv(&uv, &remove("ruff", DepKind::Dev, Some("lint"))),
        "uv | remove | ruff | --group | lint"
    );
    assert_eq!(
        argv(&uv, &remove("matplotlib", DepKind::Optional, Some("plot"))),
        "uv | remove | matplotlib | --optional | plot"
    );
    assert_eq!(
        argv(&uv, &update("httpx")),
        "uv | lock | --upgrade-package | httpx"
    );
    assert_eq!(
        argv(&uv, &PackageAction::UpdateAll),
        "uv | lock | --upgrade"
    );

    let poetry = manifest(Manager::Poetry, "pyproject.toml");
    assert_eq!(
        argv(&poetry, &add("fastapi", Some("^0.109"), false)),
        "poetry | add | fastapi@^0.109"
    );
    assert_eq!(
        argv(&poetry, &add("pytest", None, true)),
        "poetry | add | pytest | --group | dev"
    );
    assert_eq!(
        argv(&poetry, &remove("pytest", DepKind::Dev, Some("dev"))),
        "poetry | remove | pytest | --group | dev"
    );
    assert_eq!(
        argv(&poetry, &update("fastapi")),
        "poetry | update | fastapi"
    );

    let pipenv = manifest(Manager::Pipenv, "Pipfile");
    assert_eq!(
        argv(&pipenv, &add("requests", Some("2.31.0"), false)),
        "pipenv | install | requests==2.31.0"
    );
    assert_eq!(
        argv(&pipenv, &add("black", None, true)),
        "pipenv | install | black | --dev"
    );
    assert_eq!(
        argv(&pipenv, &remove("black", DepKind::Dev, None)),
        "pipenv | uninstall | black"
    );
    assert_eq!(
        argv(&pipenv, &update("requests")),
        "pipenv | update | requests"
    );

    let mut pip = manifest(Manager::Pip, "requirements-dev.txt");
    assert_eq!(
        argv(&pip, &add("flask", None, false)),
        "python3 | -m | pip | install | flask"
    );
    assert_eq!(
        argv_on(&pip, &add("flask", None, false), true),
        "python | -m | pip | install | flask"
    );
    pip.python = Some(PathBuf::from("/work/proj/.venv/bin/python"));
    assert_eq!(
        argv(&pip, &remove("flask", DepKind::Normal, None)),
        "/work/proj/.venv/bin/python | -m | pip | uninstall | -y | flask"
    );
    assert_eq!(
        argv(&pip, &update("flask")),
        "/work/proj/.venv/bin/python | -m | pip | install | -U | flask"
    );
    assert_eq!(
        argv(&pip, &PackageAction::UpdateAll),
        "/work/proj/.venv/bin/python | -m | pip | install | -U | -r | requirements-dev.txt"
    );
    let cmd = action_command(&pip, &update("flask"), false).unwrap();
    assert_eq!(cmd.display, ".venv/bin/python -m pip install -U flask");
}

#[test]
fn go_composer_bundler_commands() {
    let go = manifest(Manager::Go, "go.mod");
    assert_eq!(
        argv(&go, &add("github.com/gin-gonic/gin", Some("v1.9.1"), false)),
        "go | get | github.com/gin-gonic/gin@v1.9.1"
    );
    assert_eq!(
        argv(
            &go,
            &remove("github.com/gin-gonic/gin", DepKind::Normal, None)
        ),
        "go | get | github.com/gin-gonic/gin@none"
    );
    assert_eq!(
        argv(&go, &update("golang.org/x/net")),
        "go | get | -u | golang.org/x/net"
    );
    assert_eq!(
        argv(&go, &PackageAction::UpdateAll),
        "go | get | -u | ./..."
    );

    let composer = manifest(Manager::Composer, "composer.json");
    assert_eq!(
        argv(&composer, &add("guzzlehttp/guzzle", Some("^7.2"), false)),
        "composer | require | guzzlehttp/guzzle:^7.2"
    );
    assert_eq!(
        argv(&composer, &add("phpunit/phpunit", None, true)),
        "composer | require | phpunit/phpunit | --dev"
    );
    assert_eq!(
        argv(&composer, &remove("phpunit/phpunit", DepKind::Dev, None)),
        "composer | remove | phpunit/phpunit | --dev"
    );
    assert_eq!(
        argv_on(&composer, &update("a/b"), true),
        "composer.bat | update | a/b"
    );

    let bundle = manifest(Manager::Bundler, "Gemfile");
    assert_eq!(
        argv(&bundle, &add("rails", Some("~>7.1"), false)),
        "bundle | add | rails | --version | ~>7.1"
    );
    assert_eq!(
        argv(&bundle, &add("rspec", None, true)),
        "bundle | add | rspec | --group | development"
    );
    assert_eq!(
        argv(&bundle, &remove("rails", DepKind::Normal, None)),
        "bundle | remove | rails"
    );
    assert_eq!(argv(&bundle, &update("rails")), "bundle | update | rails");
}

#[test]
fn bad_names_and_versions_are_refused() {
    let npm = manifest(Manager::Npm, "package.json");
    for name in [
        "--registry=http://evil",
        "-g",
        "a;rm -rf /",
        "$(id)",
        "a b",
        "x|y",
        "",
    ] {
        assert!(
            action_command(&npm, &add(name, None, false), false).is_err(),
            "{name:?}"
        );
        assert!(
            action_command(&npm, &update(name), false).is_err(),
            "{name:?}"
        );
        assert!(
            action_command(&npm, &remove(name, DepKind::Normal, None), false).is_err(),
            "{name:?}"
        );
    }
    assert!(action_command(&npm, &add("a", Some("1;x"), false), false).is_err());
    assert!(action_command(&npm, &add("a", Some("--tag"), false), false).is_err());
    // cmd.exe would read `^` and `>` in a batch shim's arguments.
    assert!(action_command(&npm, &add("a", Some("^1.2"), false), true).is_err());
    assert!(action_command(&npm, &add("a", Some("^1.2"), false), false).is_ok());
    let uv = manifest(Manager::Uv, "pyproject.toml");
    assert!(action_command(&uv, &remove("x", DepKind::Dev, Some("--all")), false).is_err());
}

#[test]
fn check_commands() {
    let check = |m: &ProjectManifest, windows: bool| {
        outdated_command(m, windows).map(|(cmd, format)| {
            let mut parts = vec![cmd.program];
            parts.extend(cmd.args);
            (parts.join(" "), format)
        })
    };
    use OutdatedFormat as F;
    let cases = [
        (
            Manager::Cargo,
            "cargo update --dry-run --verbose",
            F::CargoDryRun,
        ),
        (Manager::Npm, "npm outdated --json", F::NpmJson),
        (Manager::Pnpm, "pnpm outdated --format json", F::NpmJson),
        (Manager::Yarn, "yarn outdated --json", F::YarnJson),
        (Manager::Bun, "bun outdated", F::Raw),
        (Manager::Uv, "uv lock --upgrade --dry-run", F::UvDryRun),
        (
            Manager::Poetry,
            "poetry show --outdated --no-ansi",
            F::PoetryText,
        ),
        (
            Manager::Pipenv,
            "pipenv run python -m pip list --outdated --format json",
            F::PipJson,
        ),
        (
            Manager::Pip,
            "python3 -m pip list --outdated --format json",
            F::PipJson,
        ),
        (Manager::Go, "go list -m -u -json all", F::GoJson),
        (
            Manager::Composer,
            "composer outdated --format=json",
            F::ComposerJson,
        ),
        (
            Manager::Bundler,
            "bundle outdated --parseable",
            F::BundlerParseable,
        ),
    ];
    for (manager, line, format) in cases {
        assert_eq!(
            check(&manifest(manager, "x"), false),
            Some((line.to_string(), format)),
            "{manager:?}"
        );
    }
    assert_eq!(
        check(&manifest(Manager::Npm, "x"), true).unwrap().0,
        "npm.cmd outdated --json"
    );
    let mut berry = manifest(Manager::Yarn, "package.json");
    berry.yarn_berry = true;
    assert_eq!(check(&berry, false), None);
}
