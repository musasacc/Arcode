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
    Uri::from_str(&text)
        .or_else(|_| {
            // Not expected (the encoder only emits valid URIs), but never
            // panic on a user path: retry with every byte outside the
            // unreserved set escaped, and an empty authority.
            log::warn!("could not build a valid URI for {}: {text}", path.display());
            Uri::from_str(&format!(
                "file:///{}",
                encode_strict(&path.to_string_lossy())
            ))
        })
        .or_else(|_| Uri::from_str("file:///"))
        .unwrap_or_else(|_| unreachable!("`file:///` is a valid URI"))
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
            encode_host(&host.to_ascii_lowercase()),
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
        let host = String::from_utf8(percent_decode(host)?).ok()?;
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
    encode_bytes(bytes, |b| {
        b.is_ascii_alphanumeric()
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
            )
    })
}

/// Percent-encodes a UNC server name for the URI authority (RFC 3986
/// `reg-name`: unreserved characters and sub-delimiters stay, everything
/// else, e.g. `@`, `:` or non-ASCII, is escaped).
fn encode_host(host: &str) -> String {
    encode_bytes(host.as_bytes(), |b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
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
            )
    })
}

/// Escapes everything except unreserved characters and `/`.
fn encode_strict(path: &str) -> String {
    let path = path.replace('\\', "/");
    encode_bytes(path.trim_start_matches('/').as_bytes(), |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/')
    })
}

fn encode_bytes(bytes: &[u8], keep: impl Fn(u8) -> bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if keep(b) {
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
mod tests;
