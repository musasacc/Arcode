//! Telling "the server is not installed" apart from other start failures.
//!
//! Two cases end up there: the command is not on `PATH` (spawning fails),
//! and a rustup proxy runs instead of the server. rustup puts a proxy for
//! every component it knows (`~/.cargo/bin/rust-analyzer`) on `PATH`,
//! installed or not; without the component the proxy prints
//! `error: Unknown binary 'rust-analyzer' in official toolchain '…'` on
//! stderr and exits, which would otherwise read as a crash.

/// Words every "not installed" reason contains (besides the
/// "not found on PATH" of a failed spawn).
const NOT_INSTALLED: &str = " is not installed";

/// Whether a start failure `reason` means the server is not installed.
pub fn is_not_installed(reason: &str) -> bool {
    reason.contains("not found on PATH") || reason.contains(NOT_INSTALLED)
}

/// The "not installed" reason for a stderr `line` of a rustup proxy whose
/// component is missing, e.g. `error: Unknown binary 'rust-analyzer' in
/// official toolchain 'stable-x86_64-unknown-linux-gnu'.`; `None` for any
/// other line.
pub fn rustup_missing_component(line: &str) -> Option<String> {
    let line = line.trim();
    let binary = if let Some(rest) = after(line, "Unknown binary '") {
        quoted(rest)?
    } else if line.contains("is not installed for the toolchain")
        || line.contains("is not applicable to the")
    {
        // Older rustup: `error: 'rust-analyzer' is not installed for the
        // toolchain '…'`, or `error: the 'rust-analyzer' binary, normally
        // provided by …, is not applicable to the '…' toolchain`.
        quoted(after(line, "'")?)?
    } else {
        return None;
    };
    if binary.is_empty() {
        return None;
    }
    let toolchain = after(line, "toolchain '")
        .and_then(quoted)
        .or_else(|| {
            // `… is not applicable to the '<toolchain>' toolchain`.
            after(line, "applicable to the '").and_then(quoted)
        })
        .map(|t| format!(" {t}"))
        .unwrap_or_default();
    let component = binary.strip_suffix(".exe").unwrap_or(binary);
    Some(format!(
        "{component}{NOT_INSTALLED} in the Rust toolchain{toolchain}: \
         run `rustup component add {component}`"
    ))
}

/// The text after the first `marker` in `text`.
fn after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.find(marker).map(|ix| &text[ix + marker.len()..])
}

/// The text up to the next `'`.
fn quoted(text: &str) -> Option<&str> {
    text.find('\'').map(|end| &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_binary_lines() {
        let reason = rustup_missing_component(
            "error: Unknown binary 'rust-analyzer' in official toolchain \
             '1.97.0-x86_64-unknown-linux-gnu'.",
        )
        .unwrap();
        assert!(is_not_installed(&reason), "{reason}");
        assert!(reason.starts_with("rust-analyzer is not installed"));
        assert!(reason.contains("1.97.0-x86_64-unknown-linux-gnu"));
        assert!(reason.contains("rustup component add rust-analyzer"));
        // Windows names the binary with `.exe`; a custom toolchain too.
        let windows = rustup_missing_component(
            "error: Unknown binary 'rust-analyzer.exe' in official toolchain 'stable-x86_64-pc-windows-msvc'.",
        )
        .unwrap();
        assert!(windows.ends_with("run `rustup component add rust-analyzer`"));
    }

    #[test]
    fn older_rustup_wording() {
        let old = rustup_missing_component(
            "error: 'rust-analyzer' is not installed for the toolchain 'stable-x86_64-unknown-linux-gnu'",
        )
        .unwrap();
        assert!(old.starts_with("rust-analyzer is not installed"));
        assert!(old.contains("stable-x86_64-unknown-linux-gnu"));
        let applicable = rustup_missing_component(
            "error: the 'rust-analyzer' binary, normally provided by the 'rust-analyzer' \
             component, is not applicable to the 'nightly-2020-01-01-x86_64' toolchain",
        )
        .unwrap();
        assert!(applicable.contains("nightly-2020-01-01-x86_64"));
    }

    #[test]
    fn other_lines_are_not_a_missing_component() {
        for line in [
            "",
            "error: something else",
            "Unknown binary",
            "info: syncing channel updates",
            "error: Unknown binary '' in official toolchain 'x'",
        ] {
            assert_eq!(rustup_missing_component(line), None, "{line}");
        }
        assert!(is_not_installed("gopls not found on PATH (install it)"));
        assert!(!is_not_installed("server closed the connection"));
    }
}
