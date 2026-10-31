//! Whether a manager's program is installed, and how to install it.

use std::path::{Path, PathBuf};

use super::Manager;

/// How to install `manager` when its program is missing.
pub fn install_hint(manager: Manager) -> &'static str {
    match manager {
        Manager::Cargo => "Install Rust with rustup (https://rustup.rs).",
        Manager::Npm => "Install Node.js (https://nodejs.org), which includes npm.",
        Manager::Pnpm => "Run `corepack enable` or `npm install -g pnpm`.",
        Manager::Yarn => "Run `corepack enable` or `npm install -g yarn`.",
        Manager::Bun => "Install Bun (https://bun.sh).",
        Manager::Uv => "Install uv (https://docs.astral.sh/uv/), e.g. `pipx install uv`.",
        Manager::Poetry => "Install Poetry, e.g. `pipx install poetry`.",
        Manager::Pipenv => "Install Pipenv, e.g. `pipx install pipenv`.",
        Manager::Pip => "Install Python 3 (https://python.org), which includes pip.",
        Manager::Go => "Install Go (https://go.dev/dl).",
        Manager::Composer => "Install Composer (https://getcomposer.org).",
        Manager::Bundler => "Run `gem install bundler` (needs Ruby).",
    }
}

/// Finds `program` like a shell would: a path is checked as is, a name is
/// looked up in `PATH` (with `PATHEXT` extensions on Windows when it has
/// none).
pub fn find_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 || path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let dirs = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) && path.extension().is_none() {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&dirs).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|p| p.is_file())
    })
}
