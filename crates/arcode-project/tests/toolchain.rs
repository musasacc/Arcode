//! Toolchain helpers: virtualenvs and version files.

mod common;

use arcode_project::toolchain::{
    default_python, detect_python_version_file, find_python_venv, python_program, venv_python,
};
use arcode_project::{detect_node_version_file, detect_python_venv, rust_toolchain_channel};
use common::{Project, python};

#[test]
fn venv_python_path_per_os() {
    let dir = std::path::Path::new("proj").join(".venv");
    let expected = if cfg!(windows) {
        dir.join("Scripts").join("python.exe")
    } else {
        dir.join("bin").join("python")
    };
    assert_eq!(venv_python(&dir), expected);
    assert_eq!(default_python(), python());
}

#[test]
fn detects_venv_by_interpreter_or_cfg() {
    let p = Project::new();
    assert_eq!(detect_python_venv(p.root()), None);
    let interpreter = p.venv("venv");
    let venv = detect_python_venv(p.root()).expect("venv");
    assert_eq!(venv.dir, p.path("venv"));
    assert_eq!(venv.python, interpreter);

    // `.venv` wins over `venv`.
    p.write(".venv/pyvenv.cfg", "");
    assert_eq!(
        detect_python_venv(p.root()).map(|v| v.dir),
        Some(p.path(".venv"))
    );

    // A plain `env` directory without venv markers is ignored.
    let q = Project::new();
    q.write("env/notes.txt", "");
    assert_eq!(detect_python_venv(q.root()), None);
}

#[test]
fn venv_search_walks_up() {
    let p = Project::new();
    let interpreter = p.venv(".venv");
    let deep = p.mkdir("pkg/sub");
    assert_eq!(
        find_python_venv(&deep, Some(p.root())).map(|v| v.python),
        Some(interpreter.clone())
    );
    assert_eq!(
        python_program(&deep, Some(p.root())),
        interpreter.to_string_lossy()
    );
    assert_eq!(python_program(&deep, Some(&deep)), python());
}

#[test]
fn version_files() {
    let p = Project::new();
    assert_eq!(detect_node_version_file(p.root()), None);
    p.write(".node-version", "20.11.0\n");
    assert_eq!(
        detect_node_version_file(p.root()).as_deref(),
        Some("20.11.0")
    );
    p.write(".nvmrc", "# comment\n\nlts/iron\n");
    assert_eq!(
        detect_node_version_file(p.root()).as_deref(),
        Some("lts/iron")
    );
    p.write(".python-version", "3.12\n");
    assert_eq!(
        detect_python_version_file(p.root()).as_deref(),
        Some("3.12")
    );
}

#[test]
fn rust_toolchain_files() {
    let p = Project::new();
    assert_eq!(rust_toolchain_channel(p.root()), None);
    p.write("rust-toolchain", "nightly-2026-01-01\n");
    assert_eq!(
        rust_toolchain_channel(p.root()).as_deref(),
        Some("nightly-2026-01-01")
    );
    p.write(
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.90\"\ncomponents = [\"clippy\"]\n",
    );
    assert_eq!(rust_toolchain_channel(p.root()).as_deref(), Some("1.90"));

    let legacy_toml = Project::new();
    legacy_toml.write("rust-toolchain", "[toolchain]\nchannel = \"beta\"\n");
    assert_eq!(
        rust_toolchain_channel(legacy_toml.root()).as_deref(),
        Some("beta")
    );
    let no_channel = Project::new();
    no_channel.write(
        "rust-toolchain.toml",
        "[toolchain]\nprofile = \"minimal\"\n",
    );
    assert_eq!(rust_toolchain_channel(no_channel.root()), None);
}

/// A venv made on another OS has no interpreter for this platform: it is
/// still detected, but never run, and does not count as installed.
#[test]
fn foreign_venv_falls_back_to_default_python() {
    let p = Project::new();
    p.write(".venv/pyvenv.cfg", "home = elsewhere\n");
    let foreign = if cfg!(windows) {
        ".venv/bin/python"
    } else {
        ".venv/Scripts/python.exe"
    };
    p.write(foreign, "");
    let venv = detect_python_venv(p.root()).expect("detected");
    assert!(!venv.has_interpreter());
    assert_eq!(python_program(p.root(), Some(p.root())), python());
    assert_eq!(
        arcode_project::toolchain::find_usable_python_venv(p.root(), Some(p.root())),
        None
    );

    let req = p.write("requirements.txt", "requests\n");
    let file = arcode_project::detect(&req).expect("detected");
    assert_eq!(file.install.program, python());
    assert_eq!(
        arcode_project::install_state(&file),
        arcode_project::PackagesState::NotInstalled
    );

    // A usable `venv` next to the foreign `.venv` is preferred.
    let interpreter = p.venv("venv");
    assert_eq!(
        python_program(p.root(), Some(p.root())),
        interpreter.to_string_lossy()
    );
}
