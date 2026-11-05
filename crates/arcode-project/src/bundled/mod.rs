//! Language servers bundled with Arcode releases.
//!
//! A release carries every server the built-in packs use (except those
//! listed as `not_bundled`), laid out by `scripts/fetch-language-servers.sh`
//! in a `language-servers` folder that holds a copy of the manifest
//! ([`Manifest`], `packaging/language-servers.toml`). [`bundle_dirs`] lists
//! where the app looks for it, [`Bundle::find`] loads the first one found.
//!
//! A server command resolves ([`resolve_server`]) to, in order:
//!
//! 1. the configured command itself when it is a path (an explicit
//!    `[lsp.<name>].command`),
//! 2. the command found on `PATH` (what the user installed),
//! 3. the bundled copy: a native program, a script run with the bundled
//!    Node.js (never a system `node`), or jdtls run with the user's Java.
//!
//! Nothing here downloads anything; loading a bundle reads one small file.

mod manifest;

use std::path::{Path, PathBuf};

pub use manifest::{
    BundledServer, Download, Install, LicenseFile, Manifest, NodeRuntime, NotBundled, PLATFORMS,
    SCHEMA, ServerCommand, Source, npm_spec,
};

use crate::packs::Os;

/// Name of the bundle folder.
pub const BUNDLE_DIR_NAME: &str = "language-servers";
/// The manifest copy inside a bundle.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// Where a bundle may be, in order: next to the executable in a release
/// prefix (`<exe>/../share/arcode/language-servers`, the layout of
/// `scripts/package.sh` on every OS), then the data folder
/// (`<data>/language-servers`), where builds from source get it by running
/// the fetch script.
pub fn bundle_dirs(exe: &Path, data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(prefix) = exe.parent().and_then(Path::parent) {
        dirs.push(prefix.join("share").join("arcode").join(BUNDLE_DIR_NAME));
    }
    if let Some(data) = data_dir {
        dirs.push(data.join(BUNDLE_DIR_NAME));
    }
    dirs
}

/// A bundle on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    dir: PathBuf,
    manifest: Manifest,
}

/// How to start a bundled server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The program to run.
    pub program: PathBuf,
    /// Arguments before the configured ones.
    pub args: Vec<String>,
    /// Set when a runtime the server needs is missing or too old (jdtls
    /// without Java 21): what to tell the user when it fails.
    pub missing_runtime: Option<String>,
}

/// What the machine provides for launching bundled servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The OS.
    pub os: Os,
    /// Whether the CPU is arm64 (jdtls has separate configurations).
    pub arm64: bool,
    /// The user's `java` (from `JAVA_HOME` or `PATH`), if any.
    pub java: Option<PathBuf>,
    /// Its major version, when the JDK says (see [`java_version`]).
    pub java_version: Option<u32>,
    /// A writable folder for server state (jdtls workspaces).
    pub state_dir: PathBuf,
}

impl LaunchContext {
    /// The context of this machine, with state under `state_dir`. Looks
    /// for Java (a `PATH` walk and a small file read): call it off the UI
    /// thread.
    pub fn current(state_dir: PathBuf) -> Self {
        let java = find_java(Os::current());
        Self {
            os: Os::current(),
            arm64: cfg!(target_arch = "aarch64"),
            java_version: java.as_deref().and_then(java_version),
            java,
            state_dir,
        }
    }
}

/// The oldest Java the bundled jdtls runs on (see the jdtls entry of
/// `packaging/language-servers.toml`).
pub const JDTLS_MIN_JAVA: u32 = 21;

/// The major version of the JDK or JRE that `java` belongs to, read from
/// the `release` file of its home folder (symlinks such as
/// `/usr/bin/java` are followed). `None` when there is no such file, as
/// with the macOS `/usr/bin/java` stub; Java is then assumed to be recent
/// enough rather than started to ask.
pub fn java_version(java: &Path) -> Option<u32> {
    let real = std::fs::canonicalize(java).ok()?;
    // <home>/bin/java, or <jdk>/jre/bin/java with the file in <jdk>.
    let home = real.parent()?.parent()?;
    [Some(home), home.parent()]
        .into_iter()
        .flatten()
        .find_map(|dir| std::fs::read_to_string(dir.join("release")).ok())
        .and_then(|text| parse_java_release(&text))
}

/// The major version in the text of a JDK `release` file
/// (`JAVA_VERSION="21.0.2"`, or `"1.8.0_402"` for Java 8).
pub fn parse_java_release(text: &str) -> Option<u32> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("JAVA_VERSION="))?;
    let mut parts = value.trim().trim_matches('"').split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// `$JAVA_HOME/bin/java`, else `java` on `PATH`.
pub fn find_java(os: Os) -> Option<PathBuf> {
    let exe = if os == Os::Windows {
        "java.exe"
    } else {
        "java"
    };
    std::env::var_os("JAVA_HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join("bin").join(exe))
        .filter(|java| java.is_file())
        .or_else(|| crate::packs::find_executable("java"))
}

/// The message for jdtls without Java. The server is resolved again when
/// it restarts, but Arcode's own environment (`JAVA_HOME`, `PATH`) only
/// changes when Arcode starts again.
pub const JAVA_MISSING: &str = "jdtls needs Java 21 or newer: install a JDK, set JAVA_HOME or put java on PATH, then restart Arcode";

/// The message for jdtls with a Java older than [`JDTLS_MIN_JAVA`].
pub fn java_too_old(java: &Path, version: u32) -> String {
    format!(
        "jdtls needs Java {JDTLS_MIN_JAVA} or newer, but {} is Java {version}: install a newer JDK, set JAVA_HOME to it or put it first on PATH, then restart Arcode",
        java.display()
    )
}

impl Bundle {
    /// A bundle with `manifest` at `dir` (for tests and tools).
    pub fn new(dir: PathBuf, manifest: Manifest) -> Self {
        Self { dir, manifest }
    }

    /// Loads the bundle in `dir`: `Ok(None)` when it has no manifest, an
    /// error when the manifest cannot be read or is of another schema.
    pub fn load(dir: &Path) -> Result<Option<Self>, String> {
        let path = dir.join(MANIFEST_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let manifest = Manifest::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if manifest.schema != SCHEMA {
            return Err(format!(
                "{}: schema {} (this build reads {SCHEMA})",
                path.display(),
                manifest.schema
            ));
        }
        Ok(Some(Self::new(dir.to_path_buf(), manifest)))
    }

    /// The first bundle in `dirs` that loads, and the problems with those
    /// that did not.
    pub fn find(dirs: &[PathBuf]) -> (Option<Self>, Vec<String>) {
        let mut problems = Vec::new();
        for dir in dirs {
            match Self::load(dir) {
                Ok(Some(bundle)) => return (Some(bundle), problems),
                Ok(None) => {}
                Err(problem) => problems.push(problem),
            }
        }
        (None, problems)
    }

    /// The bundle folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Whether the bundle has `command` installed (its files exist; a
    /// server left out at packaging time, such as gopls without Go, does
    /// not count).
    pub fn provides(&self, command: &str, os: Os) -> bool {
        self.manifest
            .command(command)
            .is_some_and(|(_, c)| self.target_exists(c, os))
    }

    fn target_exists(&self, command: &ServerCommand, os: Os) -> bool {
        if let Some(exec) = &command.exec {
            self.native(exec, os).is_file()
        } else if let Some(script) = &command.node {
            self.dir.join(script).is_file() && self.node(os).is_file()
        } else if let Some(dir) = &command.java {
            jdtls_launcher(&self.dir.join(dir)).is_some()
        } else {
            false
        }
    }

    /// The bundled Node.js executable.
    pub fn node(&self, os: Os) -> PathBuf {
        match os {
            Os::Windows => self.dir.join("node").join("node.exe"),
            Os::Linux | Os::MacOs => self.dir.join("node").join("bin").join("node"),
        }
    }

    fn native(&self, relative: &str, os: Os) -> PathBuf {
        let path = self.dir.join(relative);
        if os == Os::Windows && path.extension().is_none() {
            path.with_extension("exe")
        } else {
            path
        }
    }

    /// How to start the bundled `command`, if the bundle has it.
    pub fn launch(&self, command: &str, cx: &LaunchContext) -> Option<Launch> {
        let (_, entry) = self.manifest.command(command)?;
        if !self.target_exists(entry, cx.os) {
            return None;
        }
        if let Some(exec) = &entry.exec {
            return Some(Launch {
                program: self.native(exec, cx.os),
                args: Vec::new(),
                missing_runtime: None,
            });
        }
        if let Some(script) = &entry.node {
            return Some(Launch {
                program: self.node(cx.os),
                args: vec![path_arg(&self.dir.join(script))],
                missing_runtime: None,
            });
        }
        let dir = self.dir.join(entry.java.as_ref()?);
        Some(jdtls_launch(&dir, cx))
    }
}

/// The equinox launcher jar of a jdtls folder.
fn jdtls_launcher(dir: &Path) -> Option<PathBuf> {
    let plugins = dir.join("plugins");
    let mut jars: Vec<PathBuf> = std::fs::read_dir(&plugins)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("org.eclipse.equinox.launcher_") && n.ends_with(".jar")
            })
        })
        .collect();
    jars.sort();
    jars.pop()
}

/// jdtls with the arguments of its own launcher script: the shared,
/// read-only configuration of the bundle cascaded under a writable one in
/// the state folder (the bundle is never written to), and one data folder
/// per workspace root (`${workspaceFolderHash}`, expanded when it starts).
fn jdtls_launch(dir: &Path, cx: &LaunchContext) -> Launch {
    let config = match (cx.os, cx.arm64) {
        (Os::Windows, _) => "config_win",
        (Os::MacOs, false) => "config_mac",
        (Os::MacOs, true) => "config_mac_arm",
        (Os::Linux, false) => "config_linux",
        (Os::Linux, true) => "config_linux_arm",
    };
    let launcher = jdtls_launcher(dir).unwrap_or_else(|| dir.join("plugins"));
    let state = cx.state_dir.join("jdtls");
    let data = state.join("${workspaceFolderHash}");
    let mut args: Vec<String> = [
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "-Dosgi.checkConfiguration=true",
    ]
    .map(String::from)
    .to_vec();
    args.push(format!(
        "-Dosgi.sharedConfiguration.area={}",
        path_arg(&dir.join(config))
    ));
    args.extend(
        [
            "-Dosgi.sharedConfiguration.area.readOnly=true",
            "-Dosgi.configuration.cascaded=true",
            "--add-modules=ALL-SYSTEM",
            "--add-opens",
            "java.base/java.util=ALL-UNNAMED",
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-jar",
        ]
        .map(String::from),
    );
    args.push(path_arg(&launcher));
    args.push("-configuration".into());
    args.push(path_arg(&state.join("configuration")));
    args.push("-data".into());
    args.push(path_arg(&data));
    let java_exe = if cx.os == Os::Windows {
        "java.exe"
    } else {
        "java"
    };
    let missing_runtime = match (&cx.java, cx.java_version) {
        (None, _) => Some(JAVA_MISSING.to_string()),
        (Some(java), Some(version)) if version < JDTLS_MIN_JAVA => {
            Some(java_too_old(java, version))
        }
        (Some(_), _) => None,
    };
    Launch {
        program: cx.java.clone().unwrap_or_else(|| PathBuf::from(java_exe)),
        args,
        missing_runtime,
    }
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Where a resolved server command comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerSource {
    /// The configuration names a path.
    Configured,
    /// Found on `PATH`.
    Path,
    /// The bundled copy.
    Bundled,
    /// Not found anywhere: starting it reports it as not installed.
    Missing,
}

/// A server command after resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The program to run.
    pub command: String,
    /// All arguments.
    pub args: Vec<String>,
    /// Where it comes from.
    pub source: ServerSource,
    /// What to tell the user if it fails to start (a missing runtime).
    pub note: Option<String>,
}

/// Resolves the configured `command` and `args`: a path as configured,
/// else `PATH` (`on_path`), else the bundle (`launch`, given the command;
/// usually [`Bundle::launch`]). Unresolved commands stay as configured.
pub fn resolve_server(
    command: &str,
    args: &[String],
    on_path: impl FnOnce(&str) -> bool,
    launch: impl FnOnce(&str) -> Option<Launch>,
) -> Resolved {
    let as_is = |source| Resolved {
        command: command.to_string(),
        args: args.to_vec(),
        source,
        note: None,
    };
    if is_path(command) {
        return as_is(ServerSource::Configured);
    }
    if on_path(command) {
        return as_is(ServerSource::Path);
    }
    match launch(command) {
        Some(launch) => Resolved {
            command: path_arg(&launch.program),
            args: launch
                .args
                .into_iter()
                .chain(args.iter().cloned())
                .collect(),
            source: ServerSource::Bundled,
            note: launch.missing_runtime,
        },
        None => as_is(ServerSource::Missing),
    }
}

/// Whether `command` names a path rather than a program to look up.
fn is_path(command: &str) -> bool {
    command.contains(['/', '\\']) || Path::new(command).is_absolute()
}

#[cfg(test)]
mod tests;
