//! Validated identifiers used by manifests.

use std::fmt;

use serde::Deserialize;

/// A plugin id: `^[a-z0-9][a-z0-9.-]{2,63}$` (docs/PLUGINS.md §3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct PluginId(String);

/// Prefix reserved for plugins that ship with Arcode.
pub const RESERVED_PREFIX: &str = "arcode.";

impl PluginId {
    /// Validates `id`.
    pub fn parse(id: &str) -> Result<Self, String> {
        let mut chars = id.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_ok =
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if first_ok && rest_ok && (3..=64).contains(&id.len()) {
            Ok(Self(id.to_owned()))
        } else {
            Err(format!(
                "invalid plugin id `{}`: use 3-64 chars of a-z, 0-9, `.` and `-`, starting with a letter or digit",
                truncate(id, 80)
            ))
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True for ids in the `arcode.` namespace, which only bundled plugins may use.
    pub fn is_reserved(&self) -> bool {
        self.0.starts_with(RESERVED_PREFIX)
    }

    /// Namespaces a manifest-local contribution id: `<plugin-id>/<local>`.
    pub fn qualify(&self, local: &str) -> QualifiedId {
        QualifiedId(format!("{}/{local}", self.0))
    }
}

impl TryFrom<String> for PluginId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A contribution id namespaced by its plugin: `<plugin-id>/<local-id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QualifiedId(String);

impl QualifiedId {
    /// Splits `"<plugin>/<local>"`; `None` if either half is invalid.
    pub fn parse(text: &str) -> Option<Self> {
        let (plugin, local) = text.split_once('/')?;
        PluginId::parse(plugin).ok()?;
        is_local_id(local).then(|| Self(text.to_owned()))
    }

    /// The full `<plugin>/<local>` text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The plugin half.
    pub fn plugin(&self) -> &str {
        self.0.split_once('/').map_or("", |(p, _)| p)
    }

    /// The manifest-local half, as passed to the guest.
    pub fn local(&self) -> &str {
        self.0.split_once('/').map_or("", |(_, l)| l)
    }
}

impl fmt::Display for QualifiedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Manifest-local contribution ids and setting keys:
/// `^[a-z0-9][a-z0-9._-]{0,63}$`.
pub fn is_local_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && id.len() <= 64
}

/// Language ids (global, shared with arcode-syntax): `^[a-z0-9][a-z0-9_+#.-]{0,31}$`.
pub fn is_language_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '#' | '.' | '-')
        })
        && id.len() <= 32
}

/// Host icon names: `^[a-z0-9-]{1,32}$`.
pub fn is_icon_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Cuts `text` to at most `max` chars for error messages.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_regex() {
        for ok in [
            "arcode.http-client",
            "abc",
            "a1.b-c",
            "0ab",
            &"a".repeat(64),
        ] {
            assert!(PluginId::parse(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "ab",
            "Abc",
            "-ab",
            ".ab",
            "a_b",
            "a b",
            "ab/c",
            "abç",
            &"a".repeat(65),
        ] {
            assert!(PluginId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reserved_prefix() {
        assert!(PluginId::parse("arcode.db-viewer").unwrap().is_reserved());
        assert!(!PluginId::parse("arcodex.tool").unwrap().is_reserved());
    }

    #[test]
    fn qualified_ids_split() {
        let q = PluginId::parse("arcode.http-client")
            .unwrap()
            .qualify("send");
        assert_eq!(q.as_str(), "arcode.http-client/send");
        assert_eq!((q.plugin(), q.local()), ("arcode.http-client", "send"));
        assert_eq!(QualifiedId::parse("arcode.http-client/send"), Some(q));
        assert_eq!(QualifiedId::parse("arcode.http-client/"), None);
        assert_eq!(QualifiedId::parse("send"), None);
    }

    #[test]
    fn local_and_language_ids() {
        assert!(is_local_id("select-environment") && is_local_id("timeout_ms") && is_local_id("a"));
        assert!(!is_local_id("") && !is_local_id("Send") && !is_local_id("a/b"));
        assert!(is_language_id("c++") && is_language_id("c#") && is_language_id("http"));
        assert!(!is_language_id("HTTP") && !is_language_id("a b"));
        assert!(is_icon_name("globe") && !is_icon_name("../x"));
    }

    #[test]
    fn truncate_counts_chars() {
        assert_eq!(truncate("héllo", 2), "hé…");
        assert_eq!(truncate("hi", 5), "hi");
    }
}
