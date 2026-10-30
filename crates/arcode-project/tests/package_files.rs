//! Package manifest detection and install commands.

mod common;

use arcode_project::{
    PackageFile, PackageKind, PackagesState, PythonManifest, detect, detect_within, install_state,
    needs_install,
};
use common::{Project, python, strs, venv_display};

fn detected(p: &Project, rel: &str) -> PackageFile {
    detect(&p.path(rel)).unwrap_or_else(|| panic!("no package file for {rel}"))
}

#[test]
fn requirements_without_venv_uses_system_python() {
    let p = Project::new();
    p.write("requirements.txt", "requests\n");
    let file = detected(&p, "requirements.txt");
    assert_eq!(file.kind, PackageKind::Python(PythonManifest::Requirements));
    assert_eq!(file.manifest, p.path("requirements.txt"));
    assert_eq!(file.project_root, p.root());
    assert_eq!(file.install.program, python());
    assert_eq!(
        file.install.args,
        strs(&["-m", "pip", "install", "-r", "requirements.txt"])
    );
    assert_eq!(file.install.cwd, p.root());
    assert_eq!(
        file.install.display,
        format!("{} -m pip install -r requirements.txt", python())
    );
}

#[test]
fn requirements_prefers_project_venv() {
    for dir in [".venv", "venv"] {
        let p = Project::new();
        p.write("requirements.txt", "");
        let interpreter = p.venv(dir);
        let file = detected(&p, "requirements.txt");
        assert_eq!(file.install.program, interpreter.to_string_lossy());
        assert_eq!(
            file.install.display,
            format!("{} -m pip install -r requirements.txt", venv_display(dir))
        );
    }
}

#[test]
fn venv_is_found_above_a_nested_manifest() {
    let p = Project::new();
    let interpreter = p.venv(".venv");
    p.write("svc/requirements-dev.txt", "");
    let file = detected(&p, "svc/requirements-dev.txt");
    assert_eq!(file.install.program, interpreter.to_string_lossy());
    assert_eq!(
        file.install.args,
        strs(&["-m", "pip", "install", "-r", "requirements-dev.txt"])
    );
    assert_eq!(file.install.cwd, p.path("svc"));
}

#[test]
fn pyproject_lockfile_precedence() {
    let cases = [
        (&["uv.lock", "poetry.lock"][..], "uv", &["sync"][..]),
        (&["poetry.lock", "pdm.lock"], "poetry", &["install"]),
        (&["pdm.lock"], "pdm", &["install"]),
    ];
    for (locks, program, args) in cases {
        let p = Project::new();
        p.write("pyproject.toml", "[project]\nname = \"x\"\n");
        for lock in locks {
            p.write(lock, "");
        }
        let file = detected(&p, "pyproject.toml");
        assert_eq!(file.kind, PackageKind::Python(PythonManifest::Pyproject));
        assert_eq!(
            (file.install.program.as_str(), file.install.args.clone()),
            (program, strs(args))
        );
    }
    let p = Project::new();
    p.write("pyproject.toml", "");
    let file = detected(&p, "pyproject.toml");
    assert_eq!(file.install.program, python());
    assert_eq!(
        file.install.args,
        strs(&["-m", "pip", "install", "-e", "."])
    );
}

#[test]
fn uv_workspace_lock_at_root() {
    let p = Project::new();
    p.write(
        "pyproject.toml",
        "[tool.uv.workspace]\nmembers = [\"pkgs/*\"]\n",
    );
    p.write("uv.lock", "");
    p.write("pkgs/a/pyproject.toml", "");
    let file = detected(&p, "pkgs/a/pyproject.toml");
    assert_eq!(file.install.display, "uv sync");
    assert_eq!(file.install.cwd, p.root());
}

#[test]
fn pipfile_uses_pipenv() {
    let p = Project::new();
    p.write("Pipfile", "");
    let file = detected(&p, "Pipfile");
    assert_eq!(file.kind, PackageKind::Python(PythonManifest::Pipfile));
    assert_eq!(file.install.display, "pipenv install");
}

#[test]
fn node_lockfile_precedence() {
    let cases: [(&[&str], &str); 7] = [
        (
            &["pnpm-lock.yaml", "yarn.lock", "package-lock.json"],
            "pnpm install",
        ),
        (&["yarn.lock", "package-lock.json"], "yarn install"),
        (&["bun.lockb"], "bun install"),
        (&["bun.lock", "package-lock.json"], "bun install"),
        (&["package-lock.json"], "npm install"),
        (&["npm-shrinkwrap.json"], "npm install"),
        (&[], "npm install"),
    ];
    for (locks, expected) in cases {
        let p = Project::new();
        p.write("package.json", "{}");
        for lock in locks {
            p.write(lock, "");
        }
        let file = detected(&p, "package.json");
        assert_eq!(file.kind, PackageKind::Npm);
        assert_eq!(file.install.display, expected, "locks {locks:?}");
        assert_eq!(file.install.cwd, p.root());
    }
}

#[test]
fn pnpm_workspace_lock_is_found_at_workspace_root() {
    let p = Project::new();
    p.write("package.json", "{\"private\": true}");
    p.write("pnpm-workspace.yaml", "packages: ['packages/*']\n");
    p.write("pnpm-lock.yaml", "");
    p.write("packages/app/package.json", "{}");
    let file = detected(&p, "packages/app/package.json");
    assert_eq!(file.manifest, p.path("packages/app/package.json"));
    assert_eq!(file.install.display, "pnpm install");
    assert_eq!(file.install.cwd, p.root());
}

#[test]
fn package_manager_field_wins_over_lockfiles() {
    let p = Project::new();
    p.write("package.json", "{\"packageManager\": \"yarn@4.1.0\"}");
    p.write("package-lock.json", "");
    assert_eq!(detected(&p, "package.json").install.display, "yarn install");
}

#[test]
fn other_ecosystems() {
    let cases = [
        ("Cargo.toml", PackageKind::Cargo, "cargo build"),
        ("go.mod", PackageKind::Go, "go mod download"),
        ("Gemfile", PackageKind::Bundler, "bundle install"),
        ("composer.json", PackageKind::Composer, "composer install"),
    ];
    for (manifest, kind, display) in cases {
        let p = Project::new();
        p.write(manifest, "");
        let file = detected(&p, manifest);
        assert_eq!(file.kind, kind);
        assert_eq!(file.install.display, display);
        assert_eq!(file.install.cwd, p.root());
    }
}

#[test]
fn source_files_map_to_nearest_manifest() {
    let p = Project::new();
    p.write("Cargo.toml", "[workspace]\n");
    p.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
    let main = p.write("crates/a/src/main.rs", "fn main() {}");
    let file = detect(&main).expect("detect");
    assert_eq!(file.manifest, p.path("crates/a/Cargo.toml"));
    assert_eq!(file.install.cwd, p.path("crates/a"));
}

#[test]
fn source_language_picks_matching_manifest() {
    let p = Project::new();
    p.write("package.json", "{}");
    p.write("pyproject.toml", "");
    p.write("Cargo.toml", "");
    let py = p.write("tools/gen.py", "");
    assert_eq!(
        detect(&py).map(|f| f.kind),
        Some(PackageKind::Python(PythonManifest::Pyproject))
    );
    let ts = p.write("web/main.ts", "");
    assert_eq!(detect(&ts).map(|f| f.kind), Some(PackageKind::Npm));
    // Unrelated files fall back to manifest precedence (Cargo first).
    let md = p.write("README.md", "");
    assert_eq!(detect(&md).map(|f| f.kind), Some(PackageKind::Cargo));
}

#[test]
fn nothing_found() {
    let p = Project::new();
    let file = p.write("notes.txt", "");
    assert_eq!(detect(&file), None);
}

#[test]
fn boundary_limits_the_walk() {
    let p = Project::new();
    p.write("package.json", "{}");
    let file = p.write("sub/index.js", "");
    assert!(detect(&file).is_some());
    assert_eq!(detect_within(&file, Some(&p.path("sub"))), None);
}

fn state(p: &Project, rel: &str) -> PackagesState {
    install_state(&detected(p, rel))
}

#[test]
fn node_install_state() {
    let p = Project::new();
    p.write("package.json", "{}");
    assert_eq!(state(&p, "package.json"), PackagesState::NotInstalled);
    assert!(needs_install(&detected(&p, "package.json")));
    p.mkdir("node_modules");
    assert_eq!(state(&p, "package.json"), PackagesState::Installed);
    assert!(!needs_install(&detected(&p, "package.json")));

    // Hoisted workspace install and Yarn PnP.
    p.write("packages/a/package.json", "{}");
    assert_eq!(
        state(&p, "packages/a/package.json"),
        PackagesState::Installed
    );
    let pnp = Project::new();
    pnp.write("package.json", "{}");
    pnp.write(".pnp.cjs", "");
    assert_eq!(state(&pnp, "package.json"), PackagesState::Installed);
}

#[test]
fn cargo_install_state_checks_target_up_to_workspace_root() {
    let p = Project::new();
    p.write("Cargo.toml", "[workspace]\n");
    p.write("crates/a/Cargo.toml", "");
    assert_eq!(
        state(&p, "crates/a/Cargo.toml"),
        PackagesState::NotInstalled
    );
    p.mkdir("target");
    assert_eq!(state(&p, "crates/a/Cargo.toml"), PackagesState::Installed);
}

#[test]
fn python_install_state() {
    let p = Project::new();
    p.write("requirements.txt", "");
    assert_eq!(state(&p, "requirements.txt"), PackagesState::NotInstalled);
    p.venv(".venv");
    assert_eq!(state(&p, "requirements.txt"), PackagesState::Installed);

    let pipenv = Project::new();
    pipenv.write("Pipfile", "");
    assert_eq!(state(&pipenv, "Pipfile"), PackagesState::Unknown);

    let poetry = Project::new();
    poetry.write("pyproject.toml", "");
    poetry.write("poetry.lock", "");
    assert_eq!(state(&poetry, "pyproject.toml"), PackagesState::Unknown);

    let uv = Project::new();
    uv.write("pyproject.toml", "");
    uv.write("uv.lock", "");
    assert_eq!(state(&uv, "pyproject.toml"), PackagesState::NotInstalled);
}

#[test]
fn go_ruby_php_install_state() {
    let go = Project::new();
    go.write("go.mod", "module x\n");
    assert_eq!(state(&go, "go.mod"), PackagesState::Unknown);
    go.mkdir("vendor");
    assert_eq!(state(&go, "go.mod"), PackagesState::Installed);

    let ruby = Project::new();
    ruby.write("Gemfile", "");
    assert_eq!(state(&ruby, "Gemfile"), PackagesState::NotInstalled);
    ruby.write("Gemfile.lock", "");
    assert_eq!(state(&ruby, "Gemfile"), PackagesState::Unknown);
    ruby.mkdir("vendor/bundle");
    assert_eq!(state(&ruby, "Gemfile"), PackagesState::Installed);

    let php = Project::new();
    php.write("composer.json", "{}");
    assert_eq!(state(&php, "composer.json"), PackagesState::NotInstalled);
    php.mkdir("vendor");
    assert_eq!(state(&php, "composer.json"), PackagesState::Installed);
}
