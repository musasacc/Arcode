//! Redirect handling for the HTTP transport. The host follows redirects
//! itself (ureq's own following is off) so consent is re-checked for every
//! hop before anything is sent there.

use std::sync::Arc;

/// Redirect statuses that are followed.
pub fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// One request on the way: the original, or a redirect target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    /// Upper-case method.
    pub method: String,
    /// Absolute URL.
    pub url: String,
    /// Request headers, as the plugin set them.
    pub headers: Vec<(String, String)>,
    /// Request body (shared between hops and the transfer thread).
    pub body: Arc<[u8]>,
}

/// `scheme://authority` of an absolute URL, and the rest.
fn split_origin(url: &str) -> Option<(&str, &str)> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    let authority_end = rest
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| scheme_end + i);
    Some((&url[..authority_end], &url[authority_end..]))
}

fn has_scheme(location: &str) -> bool {
    let Some(colon) = location.find(':') else {
        return false;
    };
    let scheme = &location[..colon];
    !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Resolves a `Location` header against the URL that answered with it.
pub fn resolve(base: &str, location: &str) -> Result<String, String> {
    let location = location.trim();
    if location.is_empty() {
        return Err("empty Location header".into());
    }
    if has_scheme(location) {
        return Ok(location.to_owned());
    }
    let (origin, rest) = split_origin(base).ok_or("redirect from a relative URL")?;
    if let Some(net_path) = location.strip_prefix("//") {
        let scheme = &origin[..origin.find("://").unwrap_or(0)];
        return Ok(format!("{scheme}://{net_path}"));
    }
    if location.starts_with('/') {
        return Ok(format!("{origin}{location}"));
    }
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    if location.starts_with('?') {
        return Ok(format!("{origin}{path}{location}"));
    }
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Ok(format!("{origin}{dir}{location}"))
}

/// Headers that must not travel to another origin.
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "cookie", "proxy-authorization", "host"];

/// The request to send to `url` after `status`: 303 (and 301/302 after a
/// POST) become a body-less GET; 307/308 keep method and body. Credentials
/// are dropped when the origin changes.
pub fn next_hop(prev: &Hop, status: u16, url: String, same_origin: bool) -> Hop {
    let to_get = match status {
        303 => prev.method != "HEAD",
        301 | 302 => prev.method == "POST",
        _ => false,
    };
    let mut headers = prev.headers.clone();
    if to_get {
        headers.retain(|(k, _)| {
            !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("content-length")
        });
    }
    if !same_origin {
        headers.retain(|(k, _)| !CREDENTIAL_HEADERS.iter().any(|c| k.eq_ignore_ascii_case(c)));
    }
    Hop {
        method: if to_get {
            "GET".into()
        } else {
            prev.method.clone()
        },
        url,
        headers,
        body: if to_get {
            Arc::from(Vec::new())
        } else {
            prev.body.clone()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_resolve_like_a_browser() {
        let base = "http://a.test:8080/api/v1/items?x=1#frag";
        let cases = [
            ("https://b.test/x", "https://b.test/x"),
            ("//c.test/y", "http://c.test/y"),
            ("/root", "http://a.test:8080/root"),
            ("other", "http://a.test:8080/api/v1/other"),
            ("?page=2", "http://a.test:8080/api/v1/items?page=2"),
            ("  /trim  ", "http://a.test:8080/trim"),
        ];
        for (location, want) in cases {
            assert_eq!(resolve(base, location).unwrap(), want, "{location}");
        }
        assert_eq!(resolve("http://h", "x").unwrap(), "http://h/x");
        assert!(resolve(base, "").is_err());
        assert_eq!(
            resolve(base, "ftp://x/").unwrap(),
            "ftp://x/",
            "rejected later"
        );
    }

    #[test]
    fn methods_and_credentials_across_hops() {
        let post = Hop {
            method: "POST".into(),
            url: "http://a.test/".into(),
            headers: vec![
                ("Authorization".into(), "secret".into()),
                ("Content-Type".into(), "application/json".into()),
                ("X-Keep".into(), "1".into()),
            ],
            body: Arc::from(b"{}".to_vec()),
        };
        let h = next_hop(&post, 307, "http://a.test/b".into(), true);
        assert_eq!((h.method.as_str(), &*h.body), ("POST", &b"{}"[..]));
        assert_eq!(h.headers.len(), 3);
        let h = next_hop(&post, 302, "http://b.test/".into(), false);
        assert_eq!((h.method.as_str(), h.body.len()), ("GET", 0));
        assert_eq!(h.headers, [("X-Keep".to_owned(), "1".to_owned())]);
        let head = Hop {
            method: "HEAD".into(),
            ..post.clone()
        };
        assert_eq!(
            next_hop(&head, 303, "http://a.test/".into(), true).method,
            "HEAD"
        );
        let put = Hop {
            method: "PUT".into(),
            ..post
        };
        assert_eq!(
            next_hop(&put, 301, "http://a.test/".into(), true).method,
            "PUT"
        );
        assert_eq!(
            next_hop(&put, 303, "http://a.test/".into(), true).method,
            "GET"
        );
        assert!(is_redirect(308) && !is_redirect(304) && !is_redirect(300));
    }
}
