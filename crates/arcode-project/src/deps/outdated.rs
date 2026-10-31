//! "Check for updates": the manager command that lists newer versions
//! (it uses the network, so it only runs on request) and parsers for its
//! machine-readable output.

use serde_json::Value;

use super::commands::{PackageCommand, program_name, python_program};
use super::{Manager, ProjectManifest};

/// A package with a newer version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutdatedPackage {
    /// Package name.
    pub name: String,
    /// The version in use, if reported.
    pub current: Option<String>,
    /// The newest version.
    pub latest: String,
}

/// How to read a check's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedFormat {
    /// `npm outdated --json` / `pnpm outdated --format json`.
    NpmJson,
    /// `yarn outdated --json` (classic): JSON lines with a table.
    YarnJson,
    /// `pip list --outdated --format json` (also uv and pipenv).
    PipJson,
    /// `go list -m -u -json all`: a stream of JSON objects.
    GoJson,
    /// `composer outdated --format=json`.
    ComposerJson,
    /// `cargo update --dry-run --verbose` (the report is on stderr).
    CargoDryRun,
    /// `uv lock --upgrade --dry-run` (the report is on stderr).
    UvDryRun,
    /// `poetry show --outdated`: columns of text.
    PoetryText,
    /// `bundle outdated --parseable`.
    BundlerParseable,
    /// Nothing to parse: show the output in a terminal.
    Raw,
}

/// `pip list --outdated` asks the index once per installed package, and
/// by default retries each failed request five times with a growing pause:
/// offline, a system Python with 90 packages took over five minutes. One
/// retry and a 10 s timeout fail fast enough.
const PIP_OUTDATED: &[&str] = &[
    "-m",
    "pip",
    "list",
    "--outdated",
    "--format",
    "json",
    "--retries",
    "1",
    "--timeout",
    "10",
];

/// npm and pnpm retry each registry request twice, waiting 10 to 60 s in
/// between (about 70 s offline); one retry after 2 to 5 s is enough here.
const NPM_OFFLINE_FAST: &[&str] = &[
    "--fetch-retries=1",
    "--fetch-retry-mintimeout=2000",
    "--fetch-retry-maxtimeout=5000",
];

/// The check command for manifest `m` and how to read its output; `None`
/// when the manager has no such command (Yarn 2+).
pub fn outdated_command(
    m: &ProjectManifest,
    windows: bool,
) -> Option<(PackageCommand, OutdatedFormat)> {
    let s = |list: &[&str]| list.iter().map(|a| (*a).to_string()).collect::<Vec<_>>();
    let (args, format) = match m.manager {
        Manager::Cargo => (
            s(&["update", "--dry-run", "--verbose"]),
            OutdatedFormat::CargoDryRun,
        ),
        Manager::Npm => (
            [s(&["outdated", "--json"]), s(NPM_OFFLINE_FAST)].concat(),
            OutdatedFormat::NpmJson,
        ),
        Manager::Pnpm => (
            [s(&["outdated", "--format", "json"]), s(NPM_OFFLINE_FAST)].concat(),
            OutdatedFormat::NpmJson,
        ),
        Manager::Yarn if m.yarn_berry => return None,
        Manager::Yarn => (s(&["outdated", "--json"]), OutdatedFormat::YarnJson),
        Manager::Bun => (s(&["outdated"]), OutdatedFormat::Raw),
        // Reads the lockfile, like Cargo's dry run; `uv pip list` would
        // list whatever environment uv finds, not the project's.
        Manager::Uv => (
            s(&["lock", "--upgrade", "--dry-run"]),
            OutdatedFormat::UvDryRun,
        ),
        Manager::Poetry => (
            s(&["show", "--outdated", "--no-ansi"]),
            OutdatedFormat::PoetryText,
        ),
        Manager::Pipenv => (
            [s(&["run", "python"]), s(PIP_OUTDATED)].concat(),
            OutdatedFormat::PipJson,
        ),
        Manager::Pip => {
            let (program, shown) = python_program(m, windows);
            let args = s(PIP_OUTDATED);
            let cmd = PackageCommand::new(program, args, &m.dir, shown.as_deref());
            return Some((cmd, OutdatedFormat::PipJson));
        }
        Manager::Go => (
            s(&["list", "-m", "-u", "-json", "all"]),
            OutdatedFormat::GoJson,
        ),
        Manager::Composer => (
            s(&["outdated", "--format=json"]),
            OutdatedFormat::ComposerJson,
        ),
        Manager::Bundler => (
            s(&["outdated", "--parseable"]),
            OutdatedFormat::BundlerParseable,
        ),
    };
    let cmd = PackageCommand::new(program_name(m.manager, windows), args, &m.dir, None);
    Some((cmd, format))
}

/// Reads a check's output. `None` when it cannot be understood (the UI
/// then shows the raw output); an empty list means everything is current.
/// Several managers exit with 1 when something is outdated, so the exit
/// status only matters when nothing could be parsed.
pub fn parse_outdated(
    format: OutdatedFormat,
    stdout: &str,
    stderr: &str,
    success: bool,
) -> Option<Vec<OutdatedPackage>> {
    let quiet = stdout.trim().is_empty();
    match format {
        OutdatedFormat::Raw => None,
        OutdatedFormat::CargoDryRun | OutdatedFormat::UvDryRun => {
            let found = dry_run_lines(stderr);
            (success || !found.is_empty()).then_some(found)
        }
        // Offline, pip warns per package on stderr and still prints `[]`
        // with status 0: that is no "everything is current".
        OutdatedFormat::PipJson if pip_unreachable(stderr) => None,
        _ if quiet => success.then(Vec::new),
        OutdatedFormat::NpmJson => npm_json(stdout),
        OutdatedFormat::YarnJson => yarn_json(stdout),
        OutdatedFormat::PipJson => pip_json(stdout),
        OutdatedFormat::GoJson => go_json(stdout),
        OutdatedFormat::ComposerJson => composer_json(stdout),
        OutdatedFormat::PoetryText => {
            let found = poetry_text(stdout);
            (success || !found.is_empty()).then_some(found)
        }
        OutdatedFormat::BundlerParseable => {
            let found = bundler_parseable(stdout);
            let only_found = stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .all(|l| l.contains(" (newest "));
            (!found.is_empty() || (success && only_found)).then_some(found)
        }
    }
}

fn entry(name: &str, current: Option<&str>, latest: Option<&str>) -> Option<OutdatedPackage> {
    let latest = latest.filter(|l| !l.is_empty())?;
    if current == Some(latest) {
        return None;
    }
    Some(OutdatedPackage {
        name: name.to_string(),
        current: current.map(str::to_string),
        latest: latest.to_string(),
    })
}

fn npm_json(stdout: &str) -> Option<Vec<OutdatedPackage>> {
    let json: Value = serde_json::from_str(stdout.trim()).ok()?;
    let map = json.as_object()?;
    // A failure (`E404`, network) is reported as `{"error": {...}}`.
    if map.get("error").is_some_and(|e| e.get("code").is_some()) {
        return None;
    }
    let mut out = Vec::new();
    for (name, info) in map {
        // npm reports one object per workspace location as an array.
        let info = match info {
            Value::Array(items) => items.first().unwrap_or(&Value::Null),
            other => other,
        };
        let get = |k: &str| info.get(k).and_then(Value::as_str);
        out.extend(entry(name, get("current"), get("latest")));
    }
    Some(out)
}

fn yarn_json(stdout: &str) -> Option<Vec<OutdatedPackage>> {
    let mut out = Vec::new();
    let mut table = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let json: Value = serde_json::from_str(line).ok()?;
        if json.get("type").and_then(Value::as_str) != Some("table") {
            continue;
        }
        table = true;
        for row in json
            .pointer("/data/body")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let col = |i: usize| row.get(i).and_then(Value::as_str);
            if let Some(name) = col(0) {
                out.extend(entry(name, col(1), col(3)));
            }
        }
    }
    table.then_some(out)
}

/// Whether pip could not reach the package index for some package.
fn pip_unreachable(stderr: &str) -> bool {
    [
        "Retrying (Retry(",
        "Could not fetch URL",
        "Failed to establish a new connection",
    ]
    .iter()
    .any(|needle| stderr.contains(needle))
}

fn pip_json(stdout: &str) -> Option<Vec<OutdatedPackage>> {
    // pip may print notices (`[notice] ...`) around the JSON array.
    let json = stdout.match_indices('[').find_map(|(i, _)| {
        let mut values = serde_json::Deserializer::from_str(&stdout[i..]).into_iter::<Value>();
        values.next()?.ok().filter(Value::is_array)
    })?;
    let items = json.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|i| {
                let get = |k: &str| i.get(k).and_then(Value::as_str);
                entry(get("name")?, get("version"), get("latest_version"))
            })
            .collect(),
    )
}

fn go_json(stdout: &str) -> Option<Vec<OutdatedPackage>> {
    let mut out = Vec::new();
    for item in serde_json::Deserializer::from_str(stdout).into_iter::<Value>() {
        let item = item.ok()?;
        if item.get("Main").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(path) = item.get("Path").and_then(Value::as_str) else {
            continue;
        };
        let current = item.get("Version").and_then(Value::as_str);
        let latest = item.pointer("/Update/Version").and_then(Value::as_str);
        out.extend(entry(path, current, latest));
    }
    Some(out)
}

fn composer_json(stdout: &str) -> Option<Vec<OutdatedPackage>> {
    let json: Value = serde_json::from_str(stdout.trim()).ok()?;
    let items = json.get("installed")?.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|i| {
                let get = |k: &str| i.get(k).and_then(Value::as_str);
                if get("latest-status") == Some("up-to-date") {
                    return None;
                }
                entry(get("name")?, get("version"), get("latest"))
            })
            .collect(),
    )
}

/// Cargo's `Updating serde v1.0.1 -> v1.0.2` and `Unchanged rand v0.7.3
/// (available: v0.8.5)` lines, uv's `Update httpx v0.27.0 -> v0.28.1`.
fn dry_run_lines(stderr: &str) -> Vec<OutdatedPackage> {
    let strip_v = |v: &str| v.trim_start_matches('v').to_string();
    let mut out = Vec::new();
    for line in stderr.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [verb, name, from, "->", to, ..]
                if matches!(*verb, "Updating" | "Upgrading" | "Update") =>
            {
                out.push(OutdatedPackage {
                    name: (*name).to_string(),
                    current: Some(strip_v(from)),
                    latest: strip_v(to),
                });
            }
            ["Unchanged", name, current, "(available:", latest, ..] => {
                out.push(OutdatedPackage {
                    name: (*name).to_string(),
                    current: Some(strip_v(current)),
                    latest: strip_v(latest.trim_end_matches(')')),
                });
            }
            _ => {}
        }
    }
    out
}

/// `name current latest description` (a `(!)` marks packages that are
/// not installed).
fn poetry_text(stdout: &str) -> Vec<OutdatedPackage> {
    let is_version = |w: &str| w.starts_with(|c: char| c.is_ascii_digit());
    stdout
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().filter(|w| *w != "(!)").collect();
            match words.as_slice() {
                [name, current, latest, ..] if is_version(current) && is_version(latest) => {
                    entry(name, Some(current), Some(latest))
                }
                _ => None,
            }
        })
        .collect()
}

/// `rack (newest 3.0.8, installed 2.2.8, requested ~> 2.2)`.
fn bundler_parseable(stdout: &str) -> Vec<OutdatedPackage> {
    stdout
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.trim().split_once(" (newest ")?;
            let rest = rest.trim_end_matches(')');
            let mut latest = None;
            let mut current = None;
            for (ix, part) in rest.split(", ").enumerate() {
                if ix == 0 {
                    latest = Some(part.trim());
                } else if let Some(v) = part.strip_prefix("installed ") {
                    current = Some(v.trim());
                }
            }
            entry(name, current, latest)
        })
        .collect()
}
