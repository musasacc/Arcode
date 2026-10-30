//! Load-time import check (§4): a component may import only the arcode
//! plugin API and a locked-down WASI 0.2, and may import
//! `arcode:plugin/http` only if its manifest declares `network`.
//!
//! The rules are pure ([`check_imports`]); [`component_imports`] lists the
//! names of a compiled component.

use std::fmt;

use crate::manifest::Capabilities;

/// The arcode package and the minor version the host has bindings for.
const ARCODE_PACKAGE: &str = "arcode:plugin";
const ARCODE_VERSION: (u64, u64) = (0, 1);

/// WASI 0.2 packages the host links (sockets are linked but always denied).
const WASI_PACKAGES: &[&str] = &["cli", "clocks", "random", "io", "filesystem", "sockets"];

/// Why a component was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// An interface outside the allow-list (e.g. `wasi:http`).
    NotAllowed(String),
    /// Built for a newer plugin API than this host.
    NewerApi {
        /// The import.
        import: String,
        /// `major.minor` the guest wants.
        version: String,
    },
    /// Imports `arcode:plugin/http` without `network` in the manifest.
    Undeclared {
        /// The import.
        import: String,
        /// The missing manifest capability.
        capability: &'static str,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAllowed(name) => {
                write!(f, "plugin imports `{name}`, which Arcode does not provide")
            }
            Self::NewerApi { import, version } => write!(
                f,
                "plugin requires a newer Arcode (plugin API {version}, import `{import}`)"
            ),
            Self::Undeclared { import, capability } => write!(
                f,
                "plugin imports `{import}` but its manifest does not declare `{capability}`"
            ),
        }
    }
}

impl std::error::Error for ImportError {}

/// A parsed `ns:pkg/iface@version` import name.
struct InterfaceName<'a> {
    package: &'a str,
    interface: &'a str,
    version: Option<(u64, u64)>,
}

fn parse(name: &str) -> Option<InterfaceName<'_>> {
    let (path, version) = match name.split_once('@') {
        Some((path, v)) => (path, Some(v)),
        None => (name, None),
    };
    let (package, interface) = path.split_once('/')?;
    if !package.contains(':') || interface.is_empty() {
        return None;
    }
    let version = match version {
        Some(v) => {
            let mut parts = v.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            Some((major, minor))
        }
        None => None,
    };
    Some(InterfaceName {
        package,
        interface,
        version,
    })
}

/// Checks every import name against the allow-list and `caps`.
pub fn check_imports<'a>(
    names: impl IntoIterator<Item = &'a str>,
    caps: &Capabilities,
) -> Result<(), ImportError> {
    for name in names {
        let not_allowed = || ImportError::NotAllowed(name.to_owned());
        let parsed = parse(name).ok_or_else(not_allowed)?;
        if parsed.package == ARCODE_PACKAGE {
            match parsed.version {
                Some(v) if v == ARCODE_VERSION => {}
                Some((major, minor)) if (major, minor) > ARCODE_VERSION => {
                    return Err(ImportError::NewerApi {
                        import: name.to_owned(),
                        version: format!("{major}.{minor}"),
                    });
                }
                _ => return Err(not_allowed()),
            }
            if parsed.interface == "http" && caps.network.is_none() {
                return Err(ImportError::Undeclared {
                    import: name.to_owned(),
                    capability: "network",
                });
            }
            continue;
        }
        let wasi_ok = parsed
            .package
            .strip_prefix("wasi:")
            .is_some_and(|pkg| WASI_PACKAGES.contains(&pkg))
            && matches!(parsed.version, Some((0, 2)));
        if !wasi_ok {
            return Err(not_allowed());
        }
    }
    Ok(())
}

/// The import names of a compiled component.
pub fn component_imports(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
) -> Vec<String> {
    component
        .component_type()
        .imports(engine)
        .map(|(name, _)| name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::NetworkCapability;

    fn with_network() -> Capabilities {
        Capabilities {
            network: Some(NetworkCapability {
                hosts: vec!["*".into()],
                background: false,
            }),
            ..Capabilities::default()
        }
    }

    /// What `echo` (rustc 1.97, wasip2) really imports.
    const TYPICAL: &[&str] = &[
        "arcode:plugin/types@0.1.0",
        "arcode:plugin/host@0.1.0",
        "arcode:plugin/editor@0.1.0",
        "arcode:plugin/ui@0.1.0",
        "wasi:cli/environment@0.2.9",
        "wasi:cli/exit@0.2.9",
        "wasi:io/error@0.2.9",
        "wasi:io/streams@0.2.9",
        "wasi:cli/stdout@0.2.9",
        "wasi:clocks/wall-clock@0.2.9",
        "wasi:filesystem/types@0.2.9",
        "wasi:filesystem/preopens@0.2.9",
        "wasi:random/random@0.2.9",
        "wasi:sockets/tcp@0.2.0",
    ];

    #[test]
    fn allowed_imports_pass() {
        check_imports(TYPICAL.iter().copied(), &Capabilities::default()).unwrap();
        let with_http = TYPICAL.iter().copied().chain(["arcode:plugin/http@0.1.0"]);
        check_imports(with_http, &with_network()).unwrap();
        // A later 0.1.x patch release is still 0.1.
        check_imports(["arcode:plugin/kv@0.1.7"], &Capabilities::default()).unwrap();
    }

    #[test]
    fn http_without_network_is_refused() {
        let err = check_imports(["arcode:plugin/http@0.1.0"], &Capabilities::default());
        assert_eq!(
            err,
            Err(ImportError::Undeclared {
                import: "arcode:plugin/http@0.1.0".into(),
                capability: "network"
            })
        );
    }

    #[test]
    fn foreign_interfaces_are_refused() {
        for name in [
            "wasi:http/outgoing-handler@0.2.0",
            "wasi:http/types@0.2.9",
            "wasi:http/outgoing-handler@0.3.0",
            "wasi:keyvalue/store@0.2.0",
            "wasi:cli/environment@0.3.0",
            "wasi:filesystem/types",
            "arcode:plugin/host",
            "arcode:other/host@0.1.0",
            "evil:pkg/iface@1.0.0",
            "bare-function",
            "",
        ] {
            assert_eq!(
                check_imports([name], &with_network()),
                Err(ImportError::NotAllowed(name.into())),
                "{name}"
            );
        }
    }

    #[test]
    fn newer_api_says_so() {
        let err = check_imports(["arcode:plugin/host@0.2.0"], &with_network()).unwrap_err();
        assert!(
            err.to_string()
                .contains("requires a newer Arcode (plugin API 0.2")
        );
        assert!(matches!(
            check_imports(["arcode:plugin/host@1.0.0"], &with_network()),
            Err(ImportError::NewerApi { .. })
        ));
        // An older API (0.0) is not supported either.
        assert!(matches!(
            check_imports(["arcode:plugin/host@0.0.1"], &with_network()),
            Err(ImportError::NotAllowed(_))
        ));
    }
}
