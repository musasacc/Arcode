//! Shared helpers for the integration tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// A temporary project root marked as a git repository (bounds upward walks).
pub struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join(".git")).expect("mkdir .git");
        Self { dir }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }

    /// Writes `content` to `rel`, creating parent directories.
    pub fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&path, content).expect("write");
        path
    }

    pub fn mkdir(&self, rel: &str) -> PathBuf {
        let path = self.path(rel);
        fs::create_dir_all(&path).expect("mkdir");
        path
    }

    /// Creates a virtualenv-looking directory `rel` with this platform's
    /// interpreter; returns the interpreter path.
    pub fn venv(&self, rel: &str) -> PathBuf {
        self.write(&format!("{rel}/pyvenv.cfg"), "home = /usr/bin\n");
        let interpreter = if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        };
        self.write(&format!("{rel}/{interpreter}"), "")
    }
}

/// Owned argument list.
pub fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// The fallback interpreter name on this platform.
pub fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// The interpreter path inside a venv, as displayed (relative, `/`).
pub fn venv_display(dir: &str) -> String {
    if cfg!(windows) {
        format!("{dir}/Scripts/python.exe")
    } else {
        format!("{dir}/bin/python")
    }
}
