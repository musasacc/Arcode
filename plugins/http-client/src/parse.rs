//! `.http` / `.rest` file parsing (JetBrains HTTP Client and VS Code REST
//! Client syntax, docs/PLUGINS.md §9).
//!
//! ```text
//! @base = https://api.example.com      file variable
//! ### Get user                          separator; text after it names the request
//! # @name user                          directive comment (also `// @name user`)
//! GET {{base}}/users/1 HTTP/1.1         request line; method defaults to GET
//!     ?fields=name                      indented lines continue the URL
//! Accept: application/json              headers until the first blank line
//!
//! {"body": true}                        body until the next separator
//! < ./more.json                         `< path` includes a file (relative to the .http file)
//! ```
//!
//! Comments (`#`, `//`) are recognised before the body only, so a body is
//! sent verbatim. Response handlers (`> {% ... %}`, `> script.js`) and
//! `<> previous-response` lines are skipped: there is no JS engine.
//! Parsing never fails; anything unrecognised before a request line is
//! ignored and a block without a request line yields no request.

/// A parsed file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpFile {
    /// `@name = value` definitions, in file order (later ones win).
    pub variables: Vec<(String, String)>,
    /// Requests, in file order.
    pub requests: Vec<RequestBlock>,
}

/// One request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestBlock {
    /// From `# @name x`, else from `### x`.
    pub name: Option<String>,
    /// Zero-based line of the request line (where the lens goes).
    pub line: u32,
    /// Lines `start..end` of the block, separator included.
    pub span: (u32, u32),
    /// Upper-case method.
    pub method: String,
    /// URL with continuation lines joined; variables unresolved.
    pub url: String,
    /// `HTTP/1.1` etc., if written.
    pub version: Option<String>,
    /// Headers in order; variables unresolved.
    pub headers: Vec<(String, String)>,
    /// Body lines; joined with `\n` when sent.
    pub body: Vec<BodyPart>,
    /// `# @no-redirect`.
    pub no_redirect: bool,
    /// `# @timeout 5` (seconds; `ms`, `s`, `m` suffixes allowed).
    pub timeout_ms: Option<u32>,
}

/// A run of body text or a file include.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    /// Literal text (may span lines).
    Text(String),
    /// `< path`: the contents of a file.
    File(String),
}

impl HttpFile {
    /// The request whose block contains zero-based `line`.
    pub fn request_at(&self, line: u32) -> Option<&RequestBlock> {
        self.requests
            .iter()
            .find(|r| (r.span.0..r.span.1).contains(&line))
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum State {
    Preamble,
    Headers,
    Body,
}

/// Parses a whole file.
pub fn parse(text: &str) -> HttpFile {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut file = HttpFile::default();
    let mut start = 0usize;
    let mut name = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("###") {
            parse_block(&lines, start, i, name.take(), &mut file);
            start = i;
            name = Some(rest.trim().to_owned()).filter(|n| !n.is_empty());
        }
    }
    parse_block(&lines, start, lines.len(), name, &mut file);
    file
}

/// Parses lines `start..end` (the first may be the `###` separator).
fn parse_block(
    lines: &[&str],
    start: usize,
    end: usize,
    name: Option<String>,
    file: &mut HttpFile,
) {
    let mut req = RequestBlock {
        name,
        span: (to_u32(start), to_u32(end)),
        ..RequestBlock::default()
    };
    let mut state = State::Preamble;
    let mut in_handler = false;
    let mut body: Vec<BodyPart> = Vec::new();
    for (i, &line) in lines.iter().enumerate().take(end).skip(start) {
        if line.starts_with("###") {
            continue;
        }
        let trimmed = line.trim();
        match state {
            State::Preamble => {
                if trimmed.is_empty() {
                } else if let Some(c) = comment(trimmed) {
                    directive(c, &mut req);
                } else if let Some(var) = variable(trimmed) {
                    file.variables.push(var);
                } else {
                    request_line(trimmed, &mut req);
                    req.line = to_u32(i);
                    state = State::Headers;
                }
            }
            State::Headers => {
                if trimmed.is_empty() {
                    state = State::Body;
                } else if comment(trimmed).is_some() {
                } else if line.starts_with([' ', '\t']) && req.headers.is_empty() {
                    req.url.push_str(trimmed);
                } else if let Some(h) = header(trimmed) {
                    req.headers.push(h);
                } else {
                    // No blank line before the body: be lenient.
                    state = State::Body;
                    body_line(line, &mut body, &mut in_handler);
                }
            }
            State::Body => body_line(line, &mut body, &mut in_handler),
        }
    }
    if state == State::Preamble {
        return;
    }
    while matches!(body.last(), Some(BodyPart::Text(t)) if t.trim().is_empty()) {
        body.pop();
    }
    req.body = merge_text(body);
    file.requests.push(req);
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The text of a `#` or `//` comment line.
fn comment(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("//"))
}

/// `@name x`, `@name=x`, `@no-redirect`, `@timeout 5 s` inside a comment.
fn directive(comment: &str, req: &mut RequestBlock) {
    let Some(d) = comment.trim().strip_prefix('@') else {
        return;
    };
    let (key, value) = match d.find(|c: char| c.is_whitespace() || c == '=') {
        Some(i) => (&d[..i], d[i..].trim_start_matches([' ', '\t', '=']).trim()),
        None => (d, ""),
    };
    match key {
        "name" if !value.is_empty() => req.name = Some(value.to_owned()),
        "no-redirect" => req.no_redirect = true,
        "timeout" => req.timeout_ms = parse_duration_ms(value),
        _ => {}
    }
}

/// `5`, `5 s`, `500 ms`, `2m` -> milliseconds (bare numbers are seconds).
pub fn parse_duration_ms(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let n: u64 = value[..digits].parse().ok()?;
    let factor = match value[digits..].trim() {
        "" | "s" => 1000,
        "ms" => 1,
        "m" => 60_000,
        _ => return None,
    };
    u32::try_from(n.checked_mul(factor)?).ok()
}

/// `@name = value` (value may be empty).
fn variable(trimmed: &str) -> Option<(String, String)> {
    let rest = trimmed.strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    valid.then(|| (name.to_owned(), value.trim().to_owned()))
}

/// `METHOD URL [HTTP/x]` or a bare `URL` (GET).
fn request_line(trimmed: &str, req: &mut RequestBlock) {
    let mut words: Vec<&str> = trimmed.split_whitespace().collect();
    match words
        .first()
        .filter(|_| words.len() > 1)
        .and_then(|w| method(w))
    {
        Some(m) => {
            req.method = m;
            words.remove(0);
        }
        None => req.method = "GET".to_owned(),
    }
    if words.len() > 1 && words.last().is_some_and(|w| w.starts_with("HTTP/")) {
        req.version = words.pop().map(str::to_owned);
    }
    req.url = words.join(" ");
}

/// Standard methods, accepted in any case.
const METHODS: [&str; 9] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

/// A method: any upper-case word (GET, PROPFIND, ...) or a standard
/// method in any case.
fn method(word: &str) -> Option<String> {
    if (2..=16).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_uppercase()) {
        return Some(word.to_owned());
    }
    let upper = word.to_ascii_uppercase();
    METHODS.contains(&upper.as_str()).then_some(upper)
}

/// `Name: value` with a token name (`{{var}}` allowed in it).
fn header(trimmed: &str) -> Option<(String, String)> {
    let (name, value) = trimmed.split_once(':')?;
    let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~{}".contains(c);
    (!name.is_empty() && name.chars().all(token))
        .then(|| (name.to_owned(), value.trim().to_owned()))
}

/// One body line: a file include, a skipped handler line, or text.
fn body_line(line: &str, body: &mut Vec<BodyPart>, in_handler: &mut bool) {
    if *in_handler {
        *in_handler = !line.contains("%}");
        return;
    }
    if let Some(rest) = line.strip_prefix("> ") {
        let rest = rest.trim();
        if rest.starts_with("{%") {
            *in_handler = !rest.contains("%}");
            return;
        }
        if rest.ends_with(".js") {
            return;
        }
    }
    if line.starts_with("<> ") {
        return;
    }
    if let Some(path) = line
        .strip_prefix('<')
        .filter(|r| r.starts_with([' ', '\t']))
    {
        body.push(BodyPart::File(path.trim().to_owned()));
        return;
    }
    body.push(BodyPart::Text(line.to_owned()));
}

/// Joins consecutive text lines with `\n`.
fn merge_text(parts: Vec<BodyPart>) -> Vec<BodyPart> {
    let mut out: Vec<BodyPart> = Vec::new();
    for part in parts {
        match (out.last_mut(), part) {
            (Some(BodyPart::Text(prev)), BodyPart::Text(next)) => {
                prev.push('\n');
                prev.push_str(&next);
            }
            (_, part) => out.push(part),
        }
    }
    out
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
