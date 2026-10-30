//! Turning a parsed request into an `http.send` request, and looking at the
//! response (content kind, sizes, raw text).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arcode_plugin::http;
use serde::Deserialize;

use crate::parse::{BodyPart, RequestBlock};
use crate::vars::{Dynamic, resolve};

/// Responses larger than this are cut by the host (`truncated`). Well below
/// the host's 50 MiB cap so the body, its text and the view fit in the
/// plugin's 64 MiB memory limit.
pub const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
/// Largest `< file` body include.
const MAX_INCLUDE_BYTES: u64 = 16 * 1024 * 1024;

/// The plugin's settings (manifest defaults merged with the user's values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Settings {
    /// Request timeout; 0 lets the host pick (30 s).
    pub timeout_ms: u64,
    /// Follow redirects unless a request says `# @no-redirect`.
    pub follow_redirects: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            follow_redirects: true,
        }
    }
}

/// Where a request is resolved.
pub struct Context<'a> {
    /// Environment variables with the file's `@name = value` on top.
    pub vars: &'a BTreeMap<String, String>,
    /// The WASI preopen (`/project`).
    pub project: &'a Path,
    /// The `.http` file, project-relative (`< file` is relative to it).
    pub doc_path: Option<&'a str>,
    /// Plugin settings.
    pub settings: Settings,
}

/// A request ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// `# @name` / `### name`.
    pub name: Option<String>,
    /// What goes to the host.
    pub request: http::Request,
}

/// Resolves variables, includes files and applies settings.
pub fn build(
    block: &RequestBlock,
    ctx: &Context<'_>,
    dynamic: &mut dyn Dynamic,
) -> Result<Prepared, String> {
    let mut sub = |text: &str| resolve(text, ctx.vars, dynamic).map_err(|e| e.message());
    let url = sub(&block.url)?;
    let mut headers = Vec::with_capacity(block.headers.len());
    for (name, value) in &block.headers {
        headers.push((sub(name)?, sub(value)?));
    }
    let url = absolute_url(&url, &headers)?;
    // Multipart delimiters must follow a CRLF (RFC 2046 §5.1.1), as the
    // JetBrains client sends them; other bodies keep `\n` line breaks.
    // Included files are sent byte for byte either way.
    let multipart = header(&headers, "content-type").is_some_and(|t| {
        t.trim_start()
            .to_ascii_lowercase()
            .starts_with("multipart/")
    });
    let eol: &[u8] = if multipart { b"\r\n" } else { b"\n" };
    let mut body = Vec::new();
    for (i, part) in block.body.iter().enumerate() {
        if i > 0 {
            body.extend_from_slice(eol);
        }
        match part {
            BodyPart::Text(text) if multipart => {
                body.extend_from_slice(sub(text)?.replace('\n', "\r\n").as_bytes());
            }
            BodyPart::Text(text) => body.extend_from_slice(sub(text)?.as_bytes()),
            BodyPart::File(path) => body.extend(include(ctx, &sub(path)?)?),
        }
    }
    let timeout = block.timeout_ms.map_or(ctx.settings.timeout_ms, u64::from);
    Ok(Prepared {
        name: block.name.clone(),
        request: http::Request {
            method: block.method.clone(),
            url,
            headers,
            body,
            timeout_ms: u32::try_from(timeout).unwrap_or(u32::MAX),
            follow_redirects: ctx.settings.follow_redirects && !block.no_redirect,
            max_response_bytes: MAX_RESPONSE_BYTES,
        },
    })
}

/// `/path` with a `Host` header becomes `http://host/path`; a URL without a
/// scheme gets `http://`.
fn absolute_url(url: &str, headers: &[(String, String)]) -> Result<String, String> {
    if url.is_empty() {
        return Err("the request has no URL".into());
    }
    if url.contains("://") {
        return Ok(url.to_owned());
    }
    if url.starts_with('/') {
        let host = header(headers, "host")
            .ok_or_else(|| format!("`{url}` needs a Host header or a full URL"))?;
        return Ok(format!("http://{host}{url}"));
    }
    Ok(format!("http://{url}"))
}

/// The bytes of a `< path` include: relative to the `.http` file, or
/// absolute under the project.
fn include(ctx: &Context<'_>, path: &str) -> Result<Vec<u8>, String> {
    let full: PathBuf = if let Some(abs) = path.strip_prefix('/') {
        let inside = Path::new("/").join(abs);
        if !inside.starts_with(ctx.project) {
            return Err(format!("cannot include `{path}`: outside the project"));
        }
        inside
    } else {
        let dir = ctx
            .doc_path
            .and_then(|p| p.rsplit_once('/'))
            .map_or(ctx.project.to_path_buf(), |(dir, _)| ctx.project.join(dir));
        dir.join(path)
    };
    let fail = |e: std::io::Error| format!("cannot include `{path}`: {e}");
    let meta = std::fs::metadata(&full).map_err(fail)?;
    if meta.len() > MAX_INCLUDE_BYTES {
        return Err(format!("cannot include `{path}`: larger than 16 MiB"));
    }
    std::fs::read(&full).map_err(fail)
}

/// The first header named `name` (case-insensitive).
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// How a response body is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// No bytes.
    Empty,
    /// The host's foldable JSON viewer.
    Json,
    /// Highlighted text, with an arcode-syntax language if one fits.
    Text(Option<&'static str>),
    /// A hex summary.
    Binary,
}

/// Picks the body view from the content type, sniffing when it is missing
/// or generic.
pub fn classify(headers: &[(String, String)], body: &[u8]) -> BodyKind {
    if body.is_empty() {
        return BodyKind::Empty;
    }
    let ct = header(headers, "content-type")
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let text = std::str::from_utf8(body).ok().filter(|t| !t.contains('\0'));
    let Some(text) = text else {
        return BodyKind::Binary;
    };
    let (kind, sub) = ct.split_once('/').unwrap_or((ct.as_str(), ""));
    if sub == "json" || sub.ends_with("+json") {
        return BodyKind::Json;
    }
    let language = match (kind, sub) {
        (_, "xml") => Some("xml"),
        (_, s) if s.ends_with("+xml") => Some("xml"),
        ("text", "html") => Some("html"),
        ("text", "css") => Some("css"),
        (_, "javascript" | "ecmascript" | "x-javascript") => Some("javascript"),
        (_, "yaml" | "x-yaml") => Some("yaml"),
        ("text", "markdown") => Some("markdown"),
        _ => None,
    };
    if language.is_some() {
        return BodyKind::Text(language);
    }
    let generic = ct.is_empty() || ct == "text/plain" || ct == "application/octet-stream";
    if generic && looks_like_json(text) {
        return BodyKind::Json;
    }
    if matches!(kind, "image" | "audio" | "video" | "font") || ct == "application/octet-stream" {
        return BodyKind::Binary;
    }
    BodyKind::Text(None)
}

/// `{...}` or `[...]` that parses as JSON.
fn looks_like_json(text: &str) -> bool {
    let t = text.trim();
    let wrapped =
        (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'));
    wrapped && serde_json::from_str::<serde::de::IgnoredAny>(t).is_ok()
}

/// `512 B`, `1.5 KB`, `3.2 MB` (1024-based).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// At most `max` bytes of `text`, cut at a char boundary; true if cut.
pub fn truncate(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// A classic hex dump of the first `max` bytes (16 per line).
pub fn hex_dump(bytes: &[u8], max: usize) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes[..bytes.len().min(max)].chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!(
            "{:08x}  {:<47}  {ascii}\n",
            row * 16,
            hex.join(" ")
        ));
    }
    if bytes.len() > max {
        out.push_str(&format!("... {} more bytes\n", bytes.len() - max));
    }
    out
}

/// The response as it came over the wire: status line, headers, body
/// (lossy UTF-8, at most `max_body` bytes).
pub fn raw_text(r: &http::Response, max_body: usize) -> String {
    let mut out = format!("{} {} {}\n", r.version, r.status, r.reason);
    for (k, v) in &r.headers {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push('\n');
    let body = String::from_utf8_lossy(&r.body);
    let (shown, cut) = truncate(&body, max_body);
    out.push_str(shown);
    if cut || r.truncated {
        out.push_str("\n\n[... truncated]");
    }
    out
}

#[cfg(test)]
#[path = "exchange_tests.rs"]
mod tests;
