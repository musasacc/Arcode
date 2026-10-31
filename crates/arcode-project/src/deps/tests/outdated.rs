//! Parsing the output of the managers' "outdated" commands.

use super::*;

fn pairs(found: Option<Vec<OutdatedPackage>>) -> Vec<String> {
    found
        .expect("parsed")
        .into_iter()
        .map(|o| {
            format!(
                "{} {} -> {}",
                o.name,
                o.current.as_deref().unwrap_or("?"),
                o.latest
            )
        })
        .collect()
}

#[test]
fn npm_and_pnpm_json() {
    // npm exits with 1 when something is outdated.
    let mut found = pairs(parse_outdated(
        OutdatedFormat::NpmJson,
        fx!("npm_outdated.json"),
        "",
        false,
    ));
    found.sort();
    assert_eq!(
        found,
        [
            "lodash 3.10.1 -> 4.17.21",
            "not-installed ? -> 1.2.0",
            "react 18.2.0 -> 18.3.1"
        ]
    );
    assert_eq!(
        pairs(parse_outdated(OutdatedFormat::NpmJson, "{}", "", true)),
        Vec::<String>::new()
    );
    assert_eq!(
        pairs(parse_outdated(OutdatedFormat::NpmJson, "", "", true)),
        Vec::<String>::new()
    );
    assert_eq!(
        parse_outdated(OutdatedFormat::NpmJson, "", "npm ERR! network", false),
        None
    );
    assert_eq!(
        parse_outdated(OutdatedFormat::NpmJson, "npm ERR! oops", "", false),
        None
    );
    let failed =
        "{\n  \"error\": {\n    \"code\": \"E404\",\n    \"summary\": \"Not Found\"\n  }\n}";
    assert_eq!(
        parse_outdated(OutdatedFormat::NpmJson, failed, "", false),
        None
    );
}

#[test]
fn yarn_pip_go_composer() {
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::YarnJson,
            fx!("yarn_outdated.jsonl"),
            "",
            false
        )),
        ["lodash 4.17.20 -> 4.17.21", "typescript 5.2.2 -> 5.3.3"]
    );
    let notice = format!(
        "{}\n[notice] A new release of pip is available\n",
        fx!("pip_outdated.json")
    );
    assert_eq!(
        pairs(parse_outdated(OutdatedFormat::PipJson, &notice, "", true)),
        ["certifi 2023.7.22 -> 2024.2.2", "Flask 2.3.3 -> 3.0.1"]
    );
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::GoJson,
            fx!("go_outdated.json"),
            "",
            true
        )),
        ["github.com/gin-gonic/gin v1.9.1 -> v1.10.0"]
    );
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::ComposerJson,
            fx!("composer_outdated.json"),
            "",
            true
        )),
        ["laravel/framework v10.41.0 -> v10.48.4"]
    );
    assert_eq!(
        parse_outdated(OutdatedFormat::GoJson, "{\"Path\": ", "", false),
        None
    );
}

#[test]
fn text_formats() {
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::CargoDryRun,
            "",
            fx!("cargo_dry_run.txt"),
            true
        )),
        [
            "anyhow 1.0.79 -> 1.0.86",
            "serde 1.0.195 -> 1.0.203",
            "rand 0.7.3 -> 0.8.5"
        ]
    );
    let uv = "Using CPython 3.13 interpreter at: /usr/bin/python3\nResolved 24 packages in 128ms\nUpdate httpx v0.27.0 -> v0.28.1\nRemove sniffio v1.3.1\n";
    assert_eq!(
        pairs(parse_outdated(OutdatedFormat::UvDryRun, "", uv, true)),
        ["httpx 0.27.0 -> 0.28.1"]
    );
    // No network: cargo fails and nothing is found.
    let offline = "    Updating crates.io index\nerror: failed to get `serde` as a dependency\n";
    assert_eq!(
        parse_outdated(OutdatedFormat::CargoDryRun, "", offline, false),
        None
    );
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::PoetryText,
            fx!("poetry_outdated.txt"),
            "",
            true
        )),
        ["fastapi 0.109.0 -> 0.110.1", "uvicorn 0.27.0 -> 0.29.0"]
    );
    assert_eq!(
        pairs(parse_outdated(
            OutdatedFormat::BundlerParseable,
            fx!("bundle_outdated.txt"),
            "",
            false
        )),
        ["rails 7.1.2 -> 7.1.3.2", "puma 6.4.0 -> 6.4.2"]
    );
    assert_eq!(
        parse_outdated(
            OutdatedFormat::BundlerParseable,
            "Could not find gem",
            "",
            false
        ),
        None
    );
    assert_eq!(
        parse_outdated(OutdatedFormat::Raw, "anything", "", true),
        None
    );
}
