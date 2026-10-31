//! `arcode:plugin/http`: every check happens here, in host code, before the
//! [`HttpTransport`] sees the request (§4):
//!
//! 1. the master switch (`settings.plugins.network`);
//! 2. a user gesture (at most `gesture_requests` per gesture), unless
//!    background network is granted;
//! 3. the URL's host against the manifest's `network.hosts`;
//! 4. per-host consent, again for every redirect target.
//!
//! Every attempt, refused or sent, goes into the plugin's network log
//! (method, host, port and status only).
//!
//! [`HttpTransport`]: crate::bridge::HttpTransport

use std::time::SystemTime;

use crate::bindings::arcode::plugin::http::{self as wit, HttpError, Request, Response};
use crate::consent::ConsentKind;
use crate::grants::Capability;
use crate::net::{NetworkEntry, NetworkOutcome};
use crate::state::HostState;

/// The default and maximum response size (§5 host caps).
pub const MAX_RESPONSE_BYTES: u64 = 50 * 1024 * 1024;

/// What the host shows and checks: taken from the parsed URL, never from
/// plugin text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// `http` or `https`.
    pub scheme: String,
    /// Lower-case host name or IP literal (IPv6 without brackets).
    pub host: String,
    /// Explicit or default port.
    pub port: u16,
}

/// Parses `scheme://host[:port][/...]` for `http`/`https`. User info is
/// refused, so `https://good.test@evil.test/` cannot mislead a prompt.
pub fn parse_origin(url: &str) -> Result<Origin, String> {
    let (scheme, rest) = url.split_once("://").ok_or("missing scheme")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return Err(format!("unsupported scheme `{scheme}`")),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err("user info in URLs is not supported".into());
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']').ok_or("unterminated IPv6 literal")?;
        (host, after.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(p) => p
            .parse::<u16>()
            .map_err(|_| format!("invalid port `{p}`"))?,
        None => default_port,
    };
    let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':');
    if host.is_empty() || !host.chars().all(valid) {
        return Err("invalid host".into());
    }
    Ok(Origin {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
    })
}

/// Longest method the host sends or shows in a prompt.
pub const MAX_METHOD_LEN: usize = 32;

/// True if `method` is an RFC 9110 `token` of at most [`MAX_METHOD_LEN`]
/// bytes: the only method text a consent prompt may show.
pub fn is_method_token(method: &str) -> bool {
    !method.is_empty()
        && method.len() <= MAX_METHOD_LEN
        && method
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// True if a manifest `network.hosts` pattern admits `host:port`.
/// Patterns: `*`, `name`, `*.suffix`, `[v6]`, each optionally `:port`.
pub fn host_allowed(patterns: &[String], host: &str, port: u16) -> bool {
    patterns.iter().any(|pattern| {
        let pattern = pattern.to_ascii_lowercase();
        let (name, want_port) = split_pattern(&pattern);
        if want_port.is_some_and(|p| p != port) {
            return false;
        }
        match name {
            "*" => true,
            n if n.starts_with("*.") => {
                let suffix = &n[1..];
                host.ends_with(suffix) && host.len() > suffix.len()
            }
            n => n.trim_start_matches('[').trim_end_matches(']') == host,
        }
    })
}

fn split_pattern(pattern: &str) -> (&str, Option<u16>) {
    let (name, port) = if pattern.starts_with('[') {
        match pattern.split_once("]:") {
            Some((n, p)) => (&pattern[..n.len() + 1], Some(p)),
            None => (pattern, None),
        }
    } else {
        match pattern.rsplit_once(':') {
            Some((n, p)) => (n, Some(p)),
            None => (pattern, None),
        }
    };
    (name, port.and_then(|p| p.parse().ok()))
}

impl HostState {
    /// Consent for one hop (the request or a redirect target).
    fn allow_host(&self, method: &str, scheme: &str, host: &str, port: u16) -> bool {
        let ctx = &self.ctx;
        let Some(net) = &ctx.manifest.capabilities.network else {
            return false;
        };
        if !host_allowed(&net.hosts, host, port) {
            return false;
        }
        let kind = ConsentKind::Network {
            method: method.to_owned(),
            scheme: scheme.to_owned(),
            host: host.to_owned(),
            port,
        };
        ctx.services.permissions.check(
            &ctx.manifest,
            ctx.project.as_ref().map(|p| p.name.as_str()),
            &Capability::Network {
                host: host.to_owned(),
            },
            kind,
            ctx.runtime.network_enabled(),
            &self.call.cancel,
        )
    }

    fn background_allowed(&self) -> bool {
        let ctx = &self.ctx;
        let mut store = crate::cancel::lock(&ctx.services.permissions.store);
        store.decide(
            ctx.manifest.id(),
            &Capability::NetworkBackground,
            &ctx.manifest.capabilities,
            ctx.runtime.network_enabled(),
        ) == crate::grants::Decision::Allow
    }

    fn send_checked(&mut self, mut req: Request) -> Result<Response, HttpError> {
        if !self.ctx.runtime.network_enabled() || self.ctx.manifest.capabilities.network.is_none() {
            return Err(HttpError::Denied);
        }
        if self.call.cancel.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        if self.call.gesture {
            if self.call.network_requests >= self.ctx.runtime.config().gesture_requests {
                return Err(HttpError::NoUserGesture);
            }
        } else if !self.background_allowed() {
            return Err(HttpError::NoUserGesture);
        }
        let origin = parse_origin(&req.url).map_err(HttpError::InvalidRequest)?;
        // Checked before any prompt: the method appears in host chrome.
        let method = req.method.to_ascii_uppercase();
        if !is_method_token(&method) {
            return Err(HttpError::InvalidRequest(
                "the method must be an HTTP token of at most 32 characters".into(),
            ));
        }
        req.method.clone_from(&method);
        if !self.allow_host(&method, &origin.scheme, &origin.host, origin.port) {
            return Err(HttpError::Denied);
        }
        self.call.network_requests += 1;
        if req.max_response_bytes == 0 || req.max_response_bytes > MAX_RESPONSE_BYTES {
            req.max_response_bytes = MAX_RESPONSE_BYTES;
        }
        let transport = self.ctx.services.http.clone();
        let cancel = self.call.cancel.clone();
        // The redirect prompt describes the hop that would be sent: its own
        // method (a 303 turns POST into GET) and the target's scheme, host
        // and port, all parsed here rather than taken from the transport.
        let mut allow_redirect = |method: &str, url: &str| {
            let method = method.to_ascii_uppercase();
            match parse_origin(url) {
                Ok(o) if is_method_token(&method) => {
                    self.allow_host(&method, &o.scheme, &o.host, o.port)
                }
                _ => false,
            }
        };
        transport.send(req, &cancel, &mut allow_redirect)
    }
}

/// A short, user-facing description of an error (network log).
pub fn describe(e: &HttpError) -> String {
    match e {
        HttpError::Denied => "denied".into(),
        HttpError::NoUserGesture => "no user gesture".into(),
        HttpError::Cancelled => "cancelled".into(),
        HttpError::Timeout => "timed out".into(),
        HttpError::InvalidRequest(m) => format!("invalid request: {m}"),
        HttpError::Dns(m) => format!("DNS: {m}"),
        HttpError::Connect(m) => format!("connect: {m}"),
        HttpError::Tls(m) => format!("TLS: {m}"),
        HttpError::Protocol(m) => format!("protocol: {m}"),
        HttpError::Io(m) => format!("I/O: {m}"),
    }
}

impl wit::Host for HostState {
    fn send(&mut self, req: Request) -> wasmtime::Result<Result<Response, HttpError>> {
        let method = super::cap(req.method.to_ascii_uppercase(), 32);
        let (host, port) = parse_origin(&req.url).map_or((String::new(), 0), |o| (o.host, o.port));
        // Consent prompts and the network are waits, not guest CPU.
        let result = self.waiting(|s| s.send_checked(req));
        let outcome = match &result {
            Ok(r) => NetworkOutcome::Status(r.status),
            Err(
                e @ (HttpError::Denied | HttpError::NoUserGesture | HttpError::InvalidRequest(_)),
            ) => NetworkOutcome::Refused(describe(e)),
            Err(e) => NetworkOutcome::Failed(describe(e)),
        };
        self.ctx.shared.network.push(NetworkEntry {
            at: SystemTime::now(),
            method,
            host,
            port,
            outcome,
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_come_from_the_parsed_url() {
        let o = parse_origin("HTTPS://API.Example.com/v1?x=1").unwrap();
        assert_eq!(
            (o.scheme.as_str(), o.host.as_str(), o.port),
            ("https", "api.example.com", 443)
        );
        let o = parse_origin("http://127.0.0.1:8080").unwrap();
        assert_eq!((o.host.as_str(), o.port), ("127.0.0.1", 8080));
        let o = parse_origin("http://[::1]:9/x").unwrap();
        assert_eq!((o.host.as_str(), o.port), ("::1", 9));
        for bad in [
            "ftp://x",
            "example.com",
            "https://good.test@evil.test/",
            "http://:80",
            "http://a b/",
            "http://x:99999",
        ] {
            assert!(parse_origin(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn methods_must_be_short_tokens() {
        for ok in ["GET", "PROPFIND", "M-SEARCH", "X_1"] {
            assert!(is_method_token(ok), "{ok}");
        }
        for bad in ["", "GET X", "GET\nPOST", "GET\"", &"A".repeat(33), "GÉT"] {
            assert!(!is_method_token(bad), "{bad:?}");
        }
    }

    #[test]
    fn manifest_host_patterns() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(host_allowed(&p(&["*"]), "anything.test", 1));
        assert!(host_allowed(&p(&["*.example.com"]), "api.example.com", 443));
        assert!(!host_allowed(&p(&["*.example.com"]), "example.com", 443));
        assert!(!host_allowed(
            &p(&["*.example.com"]),
            "evilexample.com",
            443
        ));
        assert!(host_allowed(&p(&["example.com:8080"]), "example.com", 8080));
        assert!(!host_allowed(&p(&["example.com:8080"]), "example.com", 80));
        assert!(host_allowed(&p(&["127.0.0.1"]), "127.0.0.1", 5555));
        assert!(host_allowed(&p(&["[::1]:9"]), "::1", 9));
        assert!(!host_allowed(&p(&["[::1]:9"]), "::1", 10));
        assert!(!host_allowed(&p(&[]), "x", 1));
    }
}
