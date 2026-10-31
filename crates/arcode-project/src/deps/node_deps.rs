//! `package.json` dependencies.

use serde_json::Value;

use super::cargo_deps::first_line;
use super::lines::json_key_line;
use super::{DepKind, DepSource, Dependency};

/// What a `package.json` declares.
#[derive(Debug, Default)]
pub(crate) struct NodeManifest {
    pub name: Option<String>,
    /// Workspace patterns (`"workspaces"`, array or `{ "packages": [] }`).
    pub workspaces: Vec<String>,
    pub deps: Vec<Dependency>,
}

const SECTIONS: [(&str, DepKind); 4] = [
    ("dependencies", DepKind::Normal),
    ("devDependencies", DepKind::Dev),
    ("optionalDependencies", DepKind::Optional),
    ("peerDependencies", DepKind::Peer),
];

/// Parses `package.json` text.
pub(crate) fn parse(text: &str) -> Result<NodeManifest, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    if !json.is_object() {
        return Err("package.json is not an object".to_string());
    }
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let workspaces = match json.get("workspaces") {
        Some(Value::Array(_)) => strings(json.get("workspaces")),
        Some(Value::Object(o)) => strings(o.get("packages")),
        _ => Vec::new(),
    };
    let mut deps = Vec::new();
    for (section, kind) in SECTIONS {
        let Some(map) = json.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (name, spec) in map {
            let spec = spec.as_str().unwrap_or("");
            let mut dep = Dependency::new(name, spec, kind);
            classify_spec(&mut dep, spec);
            dep.line = json_key_line(text, section, name);
            deps.push(dep);
        }
    }
    Ok(NodeManifest {
        name: json.get("name").and_then(Value::as_str).map(str::to_string),
        workspaces,
        deps,
    })
}

/// Sets the source (and the real package of an `npm:` alias) from a
/// version spec.
fn classify_spec(dep: &mut Dependency, spec: &str) {
    let s = spec.trim();
    if let Some(alias) = s.strip_prefix("npm:") {
        // `npm:real-name@^1.0` (the name may be scoped).
        match alias.rfind('@').filter(|&i| i > 0) {
            Some(i) => {
                dep.package = alias[..i].to_string();
                dep.requested = alias[i + 1..].to_string();
            }
            None => dep.package = alias.to_string(),
        }
        return;
    }
    dep.source = if s.starts_with("workspace:") {
        DepSource::Workspace
    } else if ["file:", "link:", "portal:"]
        .iter()
        .any(|p| s.starts_with(p))
        || s.starts_with("./")
        || s.starts_with("../")
    {
        DepSource::Path(s.split_once(':').map_or(s, |(_, p)| p).to_string())
    } else if s.starts_with("git")
        || ["github:", "gitlab:", "bitbucket:"]
            .iter()
            .any(|p| s.starts_with(p))
        || s.ends_with(".git")
        || is_github_shorthand(s)
    {
        DepSource::Git(s.to_string())
    } else if s.starts_with("http://") || s.starts_with("https://") {
        DepSource::Url(s.to_string())
    } else {
        DepSource::Registry
    };
}

/// `user/repo` or `user/repo#ref`.
fn is_github_shorthand(s: &str) -> bool {
    let base = s.split('#').next().unwrap_or("");
    let mut parts = base.split('/');
    let (Some(user), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ok = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    ok(user) && ok(repo) && !user.starts_with('@')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_classified() {
        let mut d = Dependency::new("x", "", DepKind::Normal);
        let cases = [
            ("^1.2.3", DepSource::Registry),
            ("workspace:*", DepSource::Workspace),
            ("file:../lib", DepSource::Path("../lib".into())),
            (
                "github:user/repo",
                DepSource::Git("github:user/repo".into()),
            ),
            ("user/repo#main", DepSource::Git("user/repo#main".into())),
            (
                "git+https://x.org/r.git",
                DepSource::Git("git+https://x.org/r.git".into()),
            ),
            (
                "https://x.org/a.tgz",
                DepSource::Url("https://x.org/a.tgz".into()),
            ),
            (">=1 <2", DepSource::Registry),
        ];
        for (spec, source) in cases {
            classify_spec(&mut d, spec);
            assert_eq!(d.source, source, "{spec}");
        }
        let mut alias = Dependency::new("str", "npm:@scope/string-width@^4", DepKind::Normal);
        classify_spec(&mut alias, "npm:@scope/string-width@^4");
        assert_eq!(alias.package, "@scope/string-width");
        assert_eq!(alias.requested, "^4");
    }
}
