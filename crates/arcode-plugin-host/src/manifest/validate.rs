//! Semantic checks that serde cannot express.
//!
//! Every string shown in host chrome (names, titles, descriptions) is
//! length-capped and free of control characters, so a manifest cannot
//! spoof or flood prompts and lists.

use std::collections::HashSet;

use super::ids::{is_icon_name, is_language_id, is_local_id, truncate};
use super::{
    Manifest, ManifestError, PluginSource, SUPPORTED_APIS, SettingContribution, SettingType,
};

const MAX_NAME: usize = 64;
const MAX_TITLE: usize = 128;
const MAX_DESCRIPTION: usize = 512;
const MAX_LIST: usize = 256;

fn invalid(message: String) -> ManifestError {
    ManifestError::Invalid(message)
}

pub(super) fn validate(m: &Manifest, source: PluginSource) -> Result<(), ManifestError> {
    let info = &m.plugin;
    if info.id.is_reserved() && source != PluginSource::Bundled {
        return Err(ManifestError::ReservedId(info.id.clone()));
    }
    if !SUPPORTED_APIS.contains(&info.api.as_str()) {
        return Err(ManifestError::UnsupportedApi {
            requested: truncate(&info.api, 16),
        });
    }
    text("plugin.name", &info.name, MAX_NAME)?;
    text("plugin.version", &info.version, 32)?;
    if let Some(d) = &info.description {
        text("plugin.description", d, MAX_DESCRIPTION)?;
    }
    entry(&info.entry)?;
    network(m)?;
    contributions(m)
}

/// Non-empty, short, no control characters.
fn text(field: &str, value: &str, max: usize) -> Result<(), ManifestError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("`{field}` must not be empty")));
    }
    if value.chars().count() > max {
        return Err(invalid(format!(
            "`{field}` is longer than {max} characters"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(format!("`{field}` contains control characters")));
    }
    Ok(())
}

/// A plain `*.wasm` file name inside the plugin folder.
fn entry(entry: &str) -> Result<(), ManifestError> {
    let plain = !entry.is_empty()
        && entry.len() <= 64
        && entry.ends_with(".wasm")
        && !entry.starts_with('.')
        && entry
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if plain {
        Ok(())
    } else {
        Err(invalid(format!(
            "`plugin.entry` must be a plain `.wasm` file name, got `{}`",
            truncate(entry, 64)
        )))
    }
}

fn network(m: &Manifest) -> Result<(), ManifestError> {
    let Some(net) = &m.capabilities.network else {
        return Ok(());
    };
    if net.hosts.is_empty() {
        return Err(invalid(
            "`capabilities.network.hosts` must list at least one host (or `*`)".into(),
        ));
    }
    if net.hosts.len() > MAX_LIST {
        return Err(invalid(format!(
            "`capabilities.network.hosts` lists more than {MAX_LIST} hosts"
        )));
    }
    for host in &net.hosts {
        if !is_host_pattern(host) {
            return Err(invalid(format!(
                "invalid network host pattern `{}`",
                truncate(host, 64)
            )));
        }
    }
    Ok(())
}

/// `*`, `name`, `*.name`, `1.2.3.4`, `[::1]`, each optionally with `:port`.
fn is_host_pattern(pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let (host, port) = match pattern.rsplit_once(':') {
        Some((h, p))
            if !h.is_empty() && !p.contains(']') && (h.starts_with('[') || !h.contains(':')) =>
        {
            (h, Some(p))
        }
        _ => (pattern, None),
    };
    if port.is_some_and(|p| p.parse::<u16>().map_or(true, |n| n == 0)) {
        return false;
    }
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<std::net::Ipv6Addr>().is_ok();
    }
    let name = host.strip_prefix("*.").unwrap_or(host);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn unique<'a>(kind: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), ManifestError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !is_local_id(id) {
            return Err(invalid(format!(
                "invalid {kind} id `{}`: use a-z, 0-9, `.`, `_`, `-`",
                truncate(id, 64)
            )));
        }
        if !seen.insert(id) {
            return Err(invalid(format!("duplicate {kind} id `{id}`")));
        }
    }
    Ok(())
}

fn language(field: &str, id: &str) -> Result<(), ManifestError> {
    if is_language_id(id) {
        Ok(())
    } else {
        Err(invalid(format!(
            "invalid language id `{}` in `{field}`",
            truncate(id, 64)
        )))
    }
}

fn contributions(m: &Manifest) -> Result<(), ManifestError> {
    let c = &m.contributes;
    let lists = [
        c.languages.len(),
        c.commands.len(),
        c.code_lenses.len(),
        c.panels.len(),
        c.openers.len(),
        c.settings.len(),
    ];
    if lists.iter().any(|&n| n > MAX_LIST) {
        return Err(invalid(format!(
            "a contribution list has more than {MAX_LIST} entries"
        )));
    }
    unique("command", c.commands.iter().map(|x| x.id.as_str()))?;
    unique("panel", c.panels.iter().map(|x| x.id.as_str()))?;
    unique("opener", c.openers.iter().map(|x| x.id.as_str()))?;
    unique("setting", c.settings.iter().map(|x| x.key.as_str()))?;

    let mut languages = HashSet::new();
    for lang in &c.languages {
        language("contributes.languages.id", &lang.id)?;
        if !languages.insert(lang.id.as_str()) {
            return Err(invalid(format!("duplicate language id `{}`", lang.id)));
        }
        text("contributes.languages.name", &lang.name, MAX_NAME)?;
        for ext in &lang.extensions {
            let ok = !ext.is_empty()
                && ext.len() <= 32
                && ext
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '+'));
            if !ok {
                return Err(invalid(format!(
                    "invalid extension `{}` (no dot, no separators)",
                    truncate(ext, 32)
                )));
            }
        }
        if let Some(token) = &lang.line_comment {
            text("contributes.languages.line-comment", token, 8)?;
        }
    }
    for cmd in &c.commands {
        text("contributes.commands.title", &cmd.title, MAX_TITLE)?;
        if let Some(key) = &cmd.key {
            text("contributes.commands.key", key, 64)?;
        }
    }
    for lens in &c.code_lenses {
        if lens.languages.is_empty() {
            return Err(invalid(
                "`contributes.code-lenses.languages` must not be empty".into(),
            ));
        }
        lens.languages
            .iter()
            .try_for_each(|l| language("contributes.code-lenses.languages", l))?;
    }
    for panel in &c.panels {
        text("contributes.panels.title", &panel.title, MAX_TITLE)?;
        if let Some(icon) = panel.icon.as_deref().filter(|i| !is_icon_name(i)) {
            return Err(invalid(format!(
                "invalid icon name `{}`",
                truncate(icon, 32)
            )));
        }
    }
    for opener in &c.openers {
        text("contributes.openers.title", &opener.title, MAX_TITLE)?;
        if opener.patterns.is_empty() {
            return Err(invalid(format!("opener `{}` has no patterns", opener.id)));
        }
        for p in &opener.patterns {
            globset::Glob::new(p).map_err(|e| {
                invalid(format!("invalid opener pattern `{}`: {e}", truncate(p, 64)))
            })?;
        }
    }
    c.settings.iter().try_for_each(setting)
}

fn setting(s: &SettingContribution) -> Result<(), ManifestError> {
    if let Some(d) = &s.description {
        text("contributes.settings.description", d, MAX_DESCRIPTION)?;
    }
    let mismatch = || {
        invalid(format!(
            "default of setting `{}` does not match its type",
            s.key
        ))
    };
    match (s.kind, &s.options) {
        (SettingType::Enum, Some(options)) if !options.is_empty() => {
            options
                .iter()
                .try_for_each(|o| text("contributes.settings.options", o, MAX_NAME))?;
            match s.default.as_str() {
                Some(d) if options.iter().any(|o| o == d) => Ok(()),
                _ => Err(invalid(format!(
                    "default of enum setting `{}` is not one of its options",
                    s.key
                ))),
            }
        }
        (SettingType::Enum, _) => Err(invalid(format!(
            "enum setting `{}` needs non-empty `options`",
            s.key
        ))),
        (_, Some(_)) => Err(invalid(format!(
            "only enum settings take `options` (setting `{}`)",
            s.key
        ))),
        (SettingType::Boolean, None) => s.default.as_bool().map(drop).ok_or_else(mismatch),
        (SettingType::Integer, None) => s.default.as_integer().map(drop).ok_or_else(mismatch),
        (SettingType::String, None) => s.default.as_str().map(drop).ok_or_else(mismatch),
    }
}

#[cfg(test)]
mod tests {
    use super::is_host_pattern;

    #[test]
    fn host_patterns() {
        for ok in [
            "*",
            "example.com",
            "*.example.com",
            "localhost:8080",
            "127.0.0.1",
            "[::1]",
            "[::1]:443",
            "a-b.c",
        ] {
            assert!(is_host_pattern(ok), "{ok}");
        }
        for bad in [
            "",
            "*.",
            "Example.com",
            "http://x",
            "a/b",
            "x:0",
            "x:99999",
            "[nope]",
            "-a.com",
            "a..b",
            "::1",
        ] {
            assert!(!is_host_pattern(bad), "{bad}");
        }
    }
}
