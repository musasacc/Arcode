//! Conversion between filesystem paths and `file://` URIs.
//!
//! The core logic works on strings/bytes and takes a `windows` flag so the
//! Windows rules (drive letters, backslashes, UNC shares) are unit-testable on
//! every platform. [`path_to_uri`] and [`uri_to_path`] pick the native rules.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// Converts an absolute path into a `file://` URI.
///
/// Relative paths are made absolute against the current directory first.
/// Spaces, non-ASCII characters and URI delimiters are percent-encoded.
pub fn path_to_uri(path: &Path) -> Uri {
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        &absolute
    };
    let text = native_path_to_uri_string(path);
    Uri::from_str(&text).expect("percent-encoded file URI is always valid")
}

/// Converts a `file://` URI back into a path. Returns `None` for other
/// schemes, malformed escapes, or hosts that cannot be represented natively.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    native_uri_to_path(uri.as_str())
}

#[cfg(unix)]
fn native_path_to_uri_string(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    format!("file://{}", encode_path(path.as_os_str().as_bytes()))
}

#[cfg(not(unix))]
fn native_path_to_uri_string(path: &Path) -> String {
    path_str_to_uri_string(&path.to_string_lossy(), cfg!(windows))
}

#[cfg(unix)]
fn native_uri_to_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let (host, path) = split_file_uri(uri)?;
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let bytes = percent_decode(path)?;
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn native_uri_to_path(uri: &str) -> Option<PathBuf> {
    uri_str_to_path_string(uri, cfg!(windows)).map(PathBuf::from)
}

/// Builds a `file://` URI string from a path string using Unix or Windows
/// rules. With `windows`, `C:\a b` becomes `file:///C:/a%20b` and
/// `\\server\share\x` becomes `file://server/share/x`.
pub fn path_str_to_uri_string(path: &str, windows: bool) -> String {
    if !windows {
        return format!("file://{}", encode_path(path.as_bytes()));
    }
    let mut p = path.replace('\\', "/");
    // Strip verbatim prefixes: `//?/C:/x` and `//?/UNC/server/share`.
    if let Some(rest) = p.strip_prefix("//?/") {
        p = match rest.strip_prefix("UNC/") {
            Some(unc) => format!("//{unc}"),
            None => rest.to_string(),
        };
    }
    if let Some(unc) = p.strip_prefix("//") {
        let (host, rest) = unc.split_once('/').unwrap_or((unc, ""));
        return format!(
            "file://{}/{}",
            host.to_ascii_lowercase(),
            encode_path(rest.as_bytes())
        );
    }
    if has_drive_letter(&p) {
        let mut chars: Vec<char> = p.chars().collect();
        chars[0] = chars[0].to_ascii_uppercase();
        p = chars.into_iter().collect();
        if p.len() == 2 {
            p.push('/');
        }
        return format!("file:///{}", encode_path(p.as_bytes()));
    }
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    format!("file://{}", encode_path(p.as_bytes()))
}

/// Parses a `file://` URI string into a path string using Unix or Windows
/// rules. Returns `None` for non-file URIs and invalid percent escapes.
pub fn uri_str_to_path_string(uri: &str, windows: bool) -> Option<String> {
    let (host, path) = split_file_uri(uri)?;
    let decoded = String::from_utf8(percent_decode(path)?).ok()?;
    if !windows {
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        return Some(decoded);
    }
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return Some(format!("\\\\{host}{}", decoded.replace('/', "\\")));
    }
    let trimmed = decoded.strip_prefix('/').unwrap_or(&decoded);
    if has_drive_letter(trimmed) {
        let mut out: String = trimmed.replace('/', "\\");
        out.replace_range(0..1, &trimmed[0..1].to_ascii_uppercase());
        if out.len() == 2 {
            out.push('\\');
        }
        return Some(out);
    }
    Some(decoded.replace('/', "\\"))
}

/// Splits `file://host/path?query#frag` into `(host, path)`; the scheme is
/// matched case-insensitively and the query/fragment are dropped.
fn split_file_uri(uri: &str) -> Option<(&str, &str)> {
    let scheme_end = uri.find(':')?;
    if !uri[..scheme_end].eq_ignore_ascii_case("file") {
        return None;
    }
    let rest = &uri[scheme_end + 1..];
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    match rest.strip_prefix("//") {
        Some(after) => {
            let slash = after.find('/').unwrap_or(after.len());
            let path = if slash == after.len() {
                "/"
            } else {
                &after[slash..]
            };
            Some((&after[..slash], path))
        }
        None if rest.starts_with('/') => Some(("", rest)),
        None => None,
    }
}

/// True for strings starting with `X:` followed by end or a separator.
fn has_drive_letter(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/')
}

/// Percent-encodes a path, keeping unreserved characters, `/`, `:`, `@` and
/// sub-delimiters other than those with special meaning in file URIs.
fn encode_path(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let keep = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'/'
                    | b':'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
            );
        if keep {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Decodes `%XX` escapes; returns `None` on malformed escapes.
fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = (*bytes.get(i + 1)? as char).to_digit(16)?;
            let lo = (*bytes.get(i + 2)? as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_paths_encode_spaces_and_unicode() {
        assert_eq!(
            path_str_to_uri_string("/home/a b/x.rs", false),
            "file:///home/a%20b/x.rs"
        );
        assert_eq!(
            path_str_to_uri_string("/tmp/\u{e4}\u{f6}/\u{1F600}.rs", false),
            "file:///tmp/%C3%A4%C3%B6/%F0%9F%98%80.rs"
        );
        assert_eq!(
            path_str_to_uri_string("/a#b?c%d", false),
            "file:///a%23b%3Fc%25d"
        );
    }

    #[test]
    fn unix_roundtrip() {
        for p in [
            "/home/a b/x.rs",
            "/tmp/\u{e4}\u{f6}/\u{1F600}.rs",
            "/a#b?c%d",
            "/",
        ] {
            let uri = path_str_to_uri_string(p, false);
            assert_eq!(uri_str_to_path_string(&uri, false).as_deref(), Some(p));
        }
    }

    #[test]
    fn windows_drive_letters() {
        assert_eq!(path_str_to_uri_string(r"C:\foo", true), "file:///C:/foo");
        assert_eq!(
            path_str_to_uri_string(r"c:\My Code\a.rs", true),
            "file:///C:/My%20Code/a.rs"
        );
        assert_eq!(path_str_to_uri_string(r"D:", true), "file:///D:/");
        assert_eq!(
            path_str_to_uri_string(r"\\?\C:\x\y", true),
            "file:///C:/x/y"
        );
        assert_eq!(
            uri_str_to_path_string("file:///C:/foo", true).as_deref(),
            Some(r"C:\foo")
        );
        assert_eq!(
            uri_str_to_path_string("file:///c%3A/foo%20bar", true).as_deref(),
            Some(r"C:\foo bar")
        );
        assert_eq!(
            uri_str_to_path_string("file:///d:", true).as_deref(),
            Some(r"D:\")
        );
    }

    #[test]
    fn windows_unc_paths() {
        assert_eq!(
            path_str_to_uri_string(r"\\Server\share\a b", true),
            "file://server/share/a%20b"
        );
        assert_eq!(
            path_str_to_uri_string(r"\\?\UNC\srv\s\f", true),
            "file://srv/s/f"
        );
        assert_eq!(
            uri_str_to_path_string("file://srv/share/f.rs", true).as_deref(),
            Some(r"\\srv\share\f.rs")
        );
    }

    #[test]
    fn windows_roundtrip() {
        for p in [r"C:\foo", r"C:\Users\J\ü ä\main.rs", r"\\srv\share\x"] {
            let uri = path_str_to_uri_string(p, true);
            assert_eq!(
                uri_str_to_path_string(&uri, true).as_deref(),
                Some(p),
                "{uri}"
            );
        }
    }

    #[test]
    fn rejects_non_file_and_bad_escapes() {
        assert_eq!(uri_str_to_path_string("untitled:Untitled-1", false), None);
        assert_eq!(uri_str_to_path_string("https://x/y", false), None);
        assert_eq!(uri_str_to_path_string("file:///a%zz", false), None);
        assert_eq!(uri_str_to_path_string("file:///a%2", false), None);
        assert_eq!(uri_str_to_path_string("file://remote/x", false), None);
    }

    #[test]
    fn accepts_variants() {
        assert_eq!(
            uri_str_to_path_string("FILE:///x", false).as_deref(),
            Some("/x")
        );
        assert_eq!(
            uri_str_to_path_string("file:/x/y", false).as_deref(),
            Some("/x/y")
        );
        assert_eq!(
            uri_str_to_path_string("file://localhost/x", false).as_deref(),
            Some("/x")
        );
        assert_eq!(
            uri_str_to_path_string("file:///x#frag", false).as_deref(),
            Some("/x")
        );
    }

    #[test]
    fn native_roundtrip_through_lsp_uri() {
        let base = std::env::temp_dir()
            .join("arcode lsp \u{e9}t\u{e9}")
            .join("m\u{fc}ll.rs");
        let uri = path_to_uri(&base);
        assert!(uri.as_str().starts_with("file:///"), "{}", uri.as_str());
        assert!(!uri.as_str().contains(' '));
        assert_eq!(uri_to_path(&uri), Some(base));
    }

    #[test]
    fn relative_paths_become_absolute() {
        let uri = path_to_uri(Path::new("some/rel.rs"));
        let back = uri_to_path(&uri).unwrap();
        assert!(back.is_absolute());
        assert!(back.ends_with("some/rel.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_unix_paths_roundtrip() {
        use std::os::unix::ffi::OsStrExt;
        let p = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/\xff\xfe.rs"));
        let uri = path_to_uri(&p);
        assert_eq!(uri.as_str(), "file:///tmp/%FF%FE.rs");
        assert_eq!(uri_to_path(&uri), Some(p));
    }
}
