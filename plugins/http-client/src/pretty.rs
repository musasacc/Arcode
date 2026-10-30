//! JSON re-indentation for "Open in editor" without building a
//! `serde_json::Value`: a `Value` needs many times the text size, which a
//! 16 MiB body cannot afford inside the 64 MiB memory limit (§9).
//!
//! The text is validated by streaming it into `IgnoredAny` (no
//! allocation), then re-indented byte by byte into an exactly sized
//! buffer. Bodies whose pretty form would exceed [`MAX_PRETTY_BYTES`] are
//! returned as received.

use serde::de::IgnoredAny;

/// Largest pretty-printed text produced; bigger bodies open unchanged.
pub const MAX_PRETTY_BYTES: usize = 24 * 1024 * 1024;

const INDENT: &[u8] = b"  ";

/// `body` re-indented with two spaces if it is valid JSON and the result
/// stays under [`MAX_PRETTY_BYTES`]; otherwise the text as received.
pub fn pretty_json(body: &[u8]) -> String {
    let raw = || String::from_utf8_lossy(body).into_owned();
    if serde_json::from_slice::<IgnoredAny>(body).is_err() {
        return raw();
    }
    let mut counter = Counter(0);
    reindent(body, &mut counter);
    let len = counter.0;
    if len > MAX_PRETTY_BYTES {
        return raw();
    }
    let mut out = Vec::with_capacity(len);
    reindent(body, &mut out);
    // Valid JSON is UTF-8 and only ASCII whitespace was changed.
    String::from_utf8(out).unwrap_or_else(|_| raw())
}

/// Where re-indented bytes go: counted first, then written.
trait Sink {
    fn put(&mut self, bytes: &[u8]);
}

struct Counter(usize);

impl Sink for Counter {
    fn put(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
}

impl Sink for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

fn newline<S: Sink>(out: &mut S, depth: usize) {
    out.put(b"\n");
    for _ in 0..depth {
        out.put(INDENT);
    }
}

/// Re-indents valid JSON `text` into `out`.
fn reindent<S: Sink>(text: &[u8], out: &mut S) {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        i += 1;
        if in_string {
            out.put(&[b]);
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => {}
            b'"' => {
                in_string = true;
                out.put(b"\"");
            }
            b'{' | b'[' => {
                out.put(&[b]);
                // Keep `{}` and `[]` on one line.
                let next = text[i..].iter().find(|c| !c.is_ascii_whitespace()).copied();
                if next == Some(if b == b'{' { b'}' } else { b']' }) {
                    continue;
                }
                depth += 1;
                newline(out, depth);
            }
            b'}' | b']' => {
                let empty = text[..i - 1]
                    .iter()
                    .rev()
                    .find(|c| !c.is_ascii_whitespace())
                    .is_some_and(|c| matches!(c, b'{' | b'['));
                if !empty {
                    depth = depth.saturating_sub(1);
                    newline(out, depth);
                }
                out.put(&[b]);
            }
            b',' => {
                out.put(b",");
                newline(out, depth);
            }
            b':' => out.put(b": "),
            other => out.put(&[other]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindents_like_serde() {
        let body = br#" {"a":[1,2,{"b":"x,\"y\":[]"}],"e":{},"f":[ ],"g":null} "#;
        let want = serde_json::to_string_pretty(
            &serde_json::from_slice::<serde_json::Value>(body).unwrap(),
        )
        .unwrap();
        // serde_json sorts nothing here (no `preserve_order` needed: one key order).
        assert_eq!(pretty_json(body), want);
        assert_eq!(pretty_json(b"not json {"), "not json {");
        assert_eq!(pretty_json(b"\"s\""), "\"s\"");
    }

    #[test]
    fn huge_bodies_stay_as_received() {
        let mut body = String::from("[");
        while body.len() < MAX_PRETTY_BYTES / 2 {
            body.push_str("{\"id\":1},");
        }
        body.push_str("0]");
        assert_eq!(pretty_json(body.as_bytes()), body);
    }
}
