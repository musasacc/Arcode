//! Checks for package names and versions typed by the user (or read from
//! a manifest) before they become command arguments.
//!
//! Commands never go through a shell, but a name starting with `-` would
//! be read as an option, and on Windows the npm, pnpm and Yarn shims are
//! batch files that `cmd.exe` interprets; so names are limited to the
//! characters package names use, and versions to those of version ranges.

/// Longest name accepted (npm's limit; others are shorter in practice).
const MAX_NAME: usize = 214;
/// Longest version or range accepted.
const MAX_VERSION: usize = 100;

/// Accepts a package name: letters, digits and `@ / . _ - + ~`, not
/// starting with `-` or `.`.
pub fn validate_package_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a package name".to_string());
    }
    if name.len() > MAX_NAME {
        return Err("The package name is too long".to_string());
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err("A package name cannot start with '-' or '.'".to_string());
    }
    if let Some(bad) = name.chars().find(|c| {
        !(c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-' | '+' | '~'))
    }) {
        return Err(format!("A package name cannot contain {}", describe(bad)));
    }
    if name.contains("..") {
        return Err("A package name cannot contain '..'".to_string());
    }
    Ok(())
}

/// Accepts a version or range: letters, digits and `. - + ~ ^ * = < > ! ,`
/// (no blanks, not starting with `-`). With `cmd_shim` (the program is a
/// Windows batch file) `< > ^ !` are refused too, since `cmd.exe` would
/// interpret them.
pub fn validate_version(version: &str, cmd_shim: bool) -> Result<(), String> {
    if version.is_empty() {
        return Err("Enter a version".to_string());
    }
    if version.len() > MAX_VERSION {
        return Err("The version is too long".to_string());
    }
    if version.starts_with('-') {
        return Err("A version cannot start with '-'".to_string());
    }
    let allowed = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(c, '.' | '-' | '+' | '~' | '*' | '=' | ',' | '_')
            || (!cmd_shim && matches!(c, '^' | '<' | '>' | '!'))
    };
    if let Some(bad) = version.chars().find(|c| !allowed(*c)) {
        return Err(format!("A version cannot contain {}", describe(bad)));
    }
    Ok(())
}

/// Accepts a Cargo target as written in `[target.'...'.dependencies]`: a
/// triple (`x86_64-pc-windows-gnu`) or a `cfg(...)` expression
/// (`cfg(target_os = "linux")`), passed to `cargo remove --target`.
pub fn validate_cargo_target(target: &str) -> Result<(), String> {
    let bad = target.is_empty()
        || target.len() > MAX_VERSION * 2
        || target.starts_with('-')
        || target.chars().any(|c| {
            !(c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '(' | ')' | '=' | '"' | ',' | ' '))
        });
    match bad {
        true => Err(format!("Unexpected Cargo target {target:?}")),
        false => Ok(()),
    }
}

fn describe(c: char) -> String {
    if c.is_whitespace() {
        "spaces".to_string()
    } else {
        format!("'{c}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in [
            "serde",
            "@types/node",
            "github.com/BurntSushi/toml",
            "symfony/console",
            "Flask_Login",
            "zope.interface",
            "c++",
        ] {
            assert!(validate_package_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-rf",
            "--registry=x",
            ".hidden",
            "a b",
            "a;b",
            "a|b",
            "$(x)",
            "`x`",
            "a&b",
            "a>b",
            "x\ny",
            "a\"b",
            "a'b",
            "../x",
            "req[socks]",
        ] {
            assert!(validate_package_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn versions() {
        for ok in [
            "1.2.3",
            "^1.2",
            "~1.2",
            ">=1,<2",
            "v1.2.3",
            "latest",
            "*",
            "1.0.0-beta.1",
        ] {
            assert!(validate_version(ok, false).is_ok(), "{ok}");
        }
        for bad in ["", "-1", "1 2", "1;x", "$(x)", "1|2", "1&2"] {
            assert!(validate_version(bad, false).is_err(), "{bad:?}");
        }
        assert!(validate_version("^1.2", true).is_err());
        assert!(validate_version(">=1", true).is_err());
        assert!(validate_version("~1.2", true).is_ok());
    }

    #[test]
    fn cargo_targets() {
        for ok in [
            "cfg(windows)",
            "cfg(target_os = \"linux\")",
            "cfg(all(unix, not(target_os = \"macos\")))",
            "x86_64-pc-windows-gnu",
        ] {
            assert!(validate_cargo_target(ok).is_ok(), "{ok}");
        }
        for bad in ["", "--target", "cfg(x);rm", "cfg($(x))", "a|b", "x\ny"] {
            assert!(validate_cargo_target(bad).is_err(), "{bad:?}");
        }
    }
}
