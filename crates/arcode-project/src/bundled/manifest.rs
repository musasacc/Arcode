//! `packaging/language-servers.toml`: the pinned list of bundled servers,
//! also copied into every bundle as `manifest.toml`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// Platforms a release is built for, as the manifest names them.
pub const PLATFORMS: [&str; 5] = [
    "linux-x64",
    "linux-arm64",
    "macos-arm64",
    "macos-x64",
    "windows-x64",
];

/// The manifest format this build reads.
pub const SCHEMA: u32 = 1;

/// The whole manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Format version ([`SCHEMA`]).
    pub schema: u32,
    /// The Node.js runtime the npm servers run on.
    pub node: NodeRuntime,
    /// The bundled servers.
    #[serde(rename = "server", default)]
    pub servers: Vec<BundledServer>,
    /// Built-in servers that are deliberately left out, with the reason.
    #[serde(default)]
    pub not_bundled: Vec<NotBundled>,
}

/// The bundled Node.js.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeRuntime {
    /// Exact version, e.g. `24.22.2`.
    pub version: String,
    /// SPDX license expression.
    pub license: String,
    /// Project homepage.
    pub homepage: String,
    /// One download per platform.
    pub platforms: BTreeMap<String, Download>,
    /// License files inside the archive.
    #[serde(default)]
    pub licenses: Vec<LicenseFile>,
}

/// Where a bundled server comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// One archive per platform.
    Archive,
    /// npm packages, installed from `packaging/npm` with its lockfile.
    Npm,
    /// Built with `go install` at packaging time.
    Go,
}

/// One bundled server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundledServer {
    /// Name, e.g. `rust-analyzer`.
    pub name: String,
    /// Exact upstream version (a release tag or `x.y.z`).
    pub version: String,
    /// LSP language ids it serves.
    pub languages: Vec<String>,
    /// SPDX license expression.
    pub license: String,
    /// Project homepage (where its source is).
    pub homepage: String,
    /// Where it comes from.
    pub source: Source,
    /// How an archive is laid out (archive servers only).
    #[serde(default)]
    pub install: Option<Install>,
    /// Downloads per platform (archive servers only).
    #[serde(default)]
    pub platforms: BTreeMap<String, Download>,
    /// Platforms without an upstream build.
    #[serde(default)]
    pub unsupported: Vec<String>,
    /// Why some platforms are unsupported.
    #[serde(default)]
    pub note: Option<String>,
    /// npm packages as `name@version` (npm servers only).
    #[serde(default)]
    pub packages: Vec<String>,
    /// Go module path (Go servers only).
    #[serde(default)]
    pub module: Option<String>,
    /// Go checksum database hash of the module (`h1:...`).
    #[serde(default)]
    pub sum: Option<String>,
    /// Minimum Go version that builds it.
    #[serde(default)]
    pub go: Option<String>,
    /// The commands the app may run.
    pub commands: Vec<ServerCommand>,
    /// License files.
    #[serde(default)]
    pub licenses: Vec<LicenseFile>,
}

/// How an archive is laid out in the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    /// Keep one executable at this path (`.exe` added on Windows).
    #[serde(default)]
    pub file: Option<String>,
    /// Unpack everything into this folder.
    #[serde(default)]
    pub dir: Option<String>,
    /// Leading folders removed from the archive's paths.
    #[serde(default)]
    pub strip: u32,
}

/// A pinned download.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    /// HTTPS URL.
    pub url: String,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    /// The executable inside a zip or tar for [`Install::file`].
    #[serde(default)]
    pub member: Option<String>,
}

/// A license file: inside the unpacked archive, or downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseFile {
    /// Path inside the unpacked archive (or Go module).
    #[serde(default)]
    pub path: Option<String>,
    /// URL of the license text.
    #[serde(default)]
    pub url: Option<String>,
    /// SHA-256 of the downloaded text.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// A command a bundled server provides and how to run it. Exactly one of
/// `exec`, `node` and `java` is set; paths are relative to the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerCommand {
    /// The command name a `[lsp.<name>]` entry runs, e.g. `pyright-langserver`.
    pub name: String,
    /// A native program (`.exe` added on Windows).
    #[serde(default)]
    pub exec: Option<String>,
    /// A script run with the bundled Node.js.
    #[serde(default)]
    pub node: Option<String>,
    /// The jdtls folder, run with the user's Java.
    #[serde(default)]
    pub java: Option<String>,
}

/// A built-in server that is not bundled.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotBundled {
    /// Server name.
    pub name: String,
    /// Its languages.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Why it is left out.
    pub reason: String,
}

impl Manifest {
    /// Parses the manifest (without [`Manifest::validate`]).
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The server providing `command`, and how to run it.
    pub fn command(&self, command: &str) -> Option<(&BundledServer, &ServerCommand)> {
        self.servers.iter().find_map(|server| {
            server
                .commands
                .iter()
                .find(|c| c.name == command)
                .map(|c| (server, c))
        })
    }

    /// Everything wrong with the manifest; empty when it is valid.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA {
            problems.push(format!("schema {} (expected {SCHEMA})", self.schema));
        }
        let node = &self.node;
        if !is_exact_version(&node.version) {
            problems.push(format!("node: version `{}` is not pinned", node.version));
        }
        check_downloads("node", &node.platforms, &[], &mut problems);
        check_licenses("node", &node.licenses, &mut problems);
        let mut names = BTreeSet::new();
        let mut commands = BTreeSet::new();
        for server in &self.servers {
            let name = server.name.as_str();
            if name.is_empty() {
                problems.push("a server has no name".into());
            }
            if !names.insert(name) {
                problems.push(format!("{name}: listed twice"));
            }
            for (field, value) in [
                ("version", &server.version),
                ("license", &server.license),
                ("homepage", &server.homepage),
            ] {
                if value.trim().is_empty() {
                    problems.push(format!("{name}: no {field}"));
                }
            }
            if !is_pinned(&server.version) {
                problems.push(format!(
                    "{name}: version `{}` is not pinned",
                    server.version
                ));
            }
            if server.languages.is_empty() {
                problems.push(format!("{name}: no languages"));
            }
            check_commands(server, &mut commands, &mut problems);
            match server.source {
                Source::Archive => check_archive(server, &mut problems),
                Source::Npm => check_npm(server, &mut problems),
                Source::Go => check_go(server, &mut problems),
            }
        }
        for entry in &self.not_bundled {
            if entry.reason.trim().is_empty() {
                problems.push(format!("{}: not bundled without a reason", entry.name));
            }
            if names.contains(entry.name.as_str()) {
                problems.push(format!("{}: both bundled and not bundled", entry.name));
            }
        }
        problems
    }
}

fn check_commands<'a>(
    server: &'a BundledServer,
    seen: &mut BTreeSet<&'a str>,
    problems: &mut Vec<String>,
) {
    let name = &server.name;
    if server.commands.is_empty() {
        problems.push(format!("{name}: no commands"));
    }
    for command in &server.commands {
        if !seen.insert(command.name.as_str()) {
            problems.push(format!("{name}: command {} is listed twice", command.name));
        }
        let targets: Vec<&String> = [&command.exec, &command.node, &command.java]
            .into_iter()
            .flatten()
            .collect();
        if targets.len() != 1 {
            problems.push(format!(
                "{name}: command {} needs exactly one of exec, node, java",
                command.name
            ));
        }
        for target in targets {
            if !is_relative_path(target) {
                problems.push(format!("{name}: `{target}` is not a relative path"));
            }
        }
        if server.source == Source::Npm && command.node.is_none() {
            problems.push(format!(
                "{name}: npm command {} must use node",
                command.name
            ));
        }
        if command
            .node
            .as_ref()
            .is_some_and(|p| !p.starts_with("lib/npm/node_modules/"))
        {
            problems.push(format!(
                "{name}: node script of {} is not in lib/npm/node_modules",
                command.name
            ));
        }
    }
}

fn check_archive(server: &BundledServer, problems: &mut Vec<String>) {
    let name = &server.name;
    match &server.install {
        Some(Install {
            file: Some(file),
            dir: None,
            ..
        }) => {
            if !is_relative_path(file) {
                problems.push(format!("{name}: install.file `{file}` is not relative"));
            }
        }
        Some(Install {
            file: None,
            dir: Some(dir),
            ..
        }) => {
            if !is_relative_path(dir) {
                problems.push(format!("{name}: install.dir `{dir}` is not relative"));
            }
        }
        _ => problems.push(format!("{name}: install needs exactly one of file, dir")),
    }
    check_downloads(name, &server.platforms, &server.unsupported, problems);
    if !server.unsupported.is_empty() && server.note.is_none() {
        problems.push(format!("{name}: unsupported platforms need a note"));
    }
    check_licenses(name, &server.licenses, problems);
}

fn check_npm(server: &BundledServer, problems: &mut Vec<String>) {
    let name = &server.name;
    if server.packages.is_empty() {
        problems.push(format!("{name}: no npm packages"));
    }
    for spec in &server.packages {
        match npm_spec(spec) {
            Some((_, version)) if is_exact_version(version) => {}
            _ => problems.push(format!("{name}: npm package `{spec}` is not pinned")),
        }
    }
    let own = format!("{name}@{}", server.version);
    if !server.packages.contains(&own) {
        problems.push(format!("{name}: packages do not include {own}"));
    }
    if !server.platforms.is_empty() || server.install.is_some() {
        problems.push(format!("{name}: npm servers have no downloads or install"));
    }
}

fn check_go(server: &BundledServer, problems: &mut Vec<String>) {
    let name = &server.name;
    if server.module.as_deref().is_none_or(str::is_empty) {
        problems.push(format!("{name}: no Go module"));
    }
    if !server.sum.as_deref().is_some_and(|s| s.starts_with("h1:")) {
        problems.push(format!("{name}: no Go checksum (h1:...)"));
    }
    if server.go.is_none() {
        problems.push(format!("{name}: no minimum Go version"));
    }
    if !server
        .version
        .strip_prefix('v')
        .is_some_and(is_exact_version)
    {
        problems.push(format!(
            "{name}: Go version `{}` is not vX.Y.Z",
            server.version
        ));
    }
    if server.commands.iter().any(|c| c.exec.is_none()) {
        problems.push(format!("{name}: Go commands must use exec"));
    }
    check_licenses(name, &server.licenses, problems);
}

fn check_downloads(
    name: &str,
    platforms: &BTreeMap<String, Download>,
    unsupported: &[String],
    problems: &mut Vec<String>,
) {
    for platform in PLATFORMS {
        let listed = platforms.get(platform);
        let skipped = unsupported.iter().any(|u| u == platform);
        match (listed, skipped) {
            (Some(_), true) => problems.push(format!(
                "{name}: {platform} is both downloaded and unsupported"
            )),
            (None, false) => problems.push(format!("{name}: no download for {platform}")),
            _ => {}
        }
    }
    for other in platforms.keys().chain(unsupported) {
        if !PLATFORMS.contains(&other.as_str()) {
            problems.push(format!("{name}: unknown platform {other}"));
        }
    }
    for (platform, download) in platforms {
        if !download.url.starts_with("https://") {
            problems.push(format!("{name}: {platform} URL is not https"));
        }
        if !is_sha256(&download.sha256) {
            problems.push(format!("{name}: {platform} has no valid sha256"));
        }
    }
}

fn check_licenses(name: &str, licenses: &[LicenseFile], problems: &mut Vec<String>) {
    if licenses.is_empty() {
        problems.push(format!("{name}: no license files"));
    }
    for license in licenses {
        match (&license.path, &license.url) {
            (Some(path), None) if is_relative_path(path) => {}
            (None, Some(url)) => {
                if !url.starts_with("https://") {
                    problems.push(format!("{name}: license URL {url} is not https"));
                }
                if !license.sha256.as_deref().is_some_and(is_sha256) {
                    problems.push(format!("{name}: license {url} has no valid sha256"));
                }
            }
            _ => problems.push(format!("{name}: a license needs a relative path or a url")),
        }
    }
}

/// `name@version` of an npm spec (scoped names start with `@`).
pub fn npm_spec(spec: &str) -> Option<(&str, &str)> {
    let at = spec.rfind('@').filter(|&i| i > 0)?;
    Some((&spec[..at], &spec[at + 1..]))
}

/// `1.2.3` (with an optional `-pre` suffix): an exact semantic version.
fn is_exact_version(version: &str) -> bool {
    let core = version.split_once('-').map_or(version, |(core, _)| core);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A version that names exactly one release: no ranges, wildcards or
/// moving names such as `latest`.
fn is_pinned(version: &str) -> bool {
    let v = version.trim();
    !v.is_empty()
        && !matches!(v, "latest" | "nightly" | "stable" | "main" | "master")
        && !v.contains(['^', '~', '*', '<', '>', '=', ' ', '|'])
        && !v.split('.').any(|p| p.eq_ignore_ascii_case("x"))
        && v.bytes().any(|b| b.is_ascii_digit())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains(':')
        && !path.split(['/', '\\']).any(|part| part == "..")
}
