//! `exchange` tests.

use super::*;
use crate::parse::parse;
use crate::test_support::TempDir;
use crate::vars::SystemDynamic;

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn build_one(src: &str, ctx: &Context<'_>) -> Result<Prepared, String> {
    let file = parse(src);
    build(&file.requests[0], ctx, &mut SystemDynamic::default())
}

fn ctx<'a>(vars: &'a BTreeMap<String, String>, project: &'a Path) -> Context<'a> {
    Context {
        vars,
        project,
        doc_path: Some("api/a.http"),
        settings: Settings::default(),
    }
}

#[test]
fn builds_a_request_with_variables_everywhere() {
    let v = vars(&[
        ("base", "http://localhost:9"),
        ("tok", "s3cret"),
        ("h", "X-Id"),
    ]);
    let src = "\
# @name create
# @timeout 2
POST {{base}}/items?q={{tok}}
Authorization: Bearer {{tok}}
{{h}}: 1

{\"token\": \"{{tok}}\"}
";
    let p = build_one(src, &ctx(&v, Path::new("/project"))).unwrap();
    assert_eq!(p.name.as_deref(), Some("create"));
    let r = &p.request;
    assert_eq!(r.method, "POST");
    assert_eq!(r.url, "http://localhost:9/items?q=s3cret");
    assert_eq!(
        r.headers,
        [
            ("Authorization".into(), "Bearer s3cret".into()),
            ("X-Id".into(), "1".into())
        ]
    );
    assert_eq!(r.body, br#"{"token": "s3cret"}"#);
    assert_eq!(r.timeout_ms, 2000);
    assert!(r.follow_redirects);
    assert_eq!(r.max_response_bytes, MAX_RESPONSE_BYTES);
}

#[test]
fn settings_and_directives() {
    let v = BTreeMap::new();
    let mut c = ctx(&v, Path::new("/project"));
    c.settings = Settings {
        timeout_ms: 1234,
        follow_redirects: true,
    };
    let r = build_one("# @no-redirect\nGET http://x", &c)
        .unwrap()
        .request;
    assert!(!r.follow_redirects);
    assert_eq!(r.timeout_ms, 1234);
    c.settings.follow_redirects = false;
    assert!(
        !build_one("GET http://x", &c)
            .unwrap()
            .request
            .follow_redirects
    );
    let s: Settings =
        serde_json::from_str(r#"{"timeout-ms": 5, "follow-redirects": false}"#).unwrap();
    assert_eq!(
        s,
        Settings {
            timeout_ms: 5,
            follow_redirects: false
        }
    );
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, Settings::default());
}

#[test]
fn urls_without_scheme_or_host() {
    let v = BTreeMap::new();
    let c = ctx(&v, Path::new("/project"));
    let url = |src: &str| build_one(src, &c).map(|p| p.request.url);
    assert_eq!(
        url("GET localhost:8080/a"),
        Ok("http://localhost:8080/a".into())
    );
    assert_eq!(
        url("GET /a\nHost: example.com"),
        Ok("http://example.com/a".into())
    );
    assert!(url("GET /a").unwrap_err().contains("Host header"));
    assert!(
        url("GET {{nope}}")
            .unwrap_err()
            .contains("unknown variable {{nope}}")
    );
}

#[test]
fn file_includes_are_relative_to_the_http_file() {
    let dir = TempDir::new("include");
    let root = dir.path();
    std::fs::create_dir_all(root.join("api")).unwrap();
    std::fs::write(root.join("api/body.json"), "{\"a\": {{x}}}").unwrap();
    let v = vars(&[("x", "1"), ("file", "body.json")]);
    let c = ctx(&v, root);
    let src = "POST http://x\n\nbefore\n< ./{{file}}\nafter\n";
    let body = build_one(src, &c).unwrap().request.body;
    assert_eq!(
        body, b"before\n{\"a\": {{x}}}\nafter",
        "file contents are verbatim"
    );
    let err = build_one("POST http://x\n\n< ./missing.json\n", &c).unwrap_err();
    assert!(err.starts_with("cannot include `./missing.json`"), "{err}");
    let err = build_one("POST http://x\n\n< /etc/passwd\n", &c).unwrap_err();
    assert!(err.contains("outside the project"), "{err}");
}

fn ct(value: &str) -> Vec<(String, String)> {
    vec![("Content-Type".into(), value.into())]
}

#[test]
fn classifies_bodies() {
    assert_eq!(classify(&ct("application/json"), b"{}"), BodyKind::Json);
    assert_eq!(
        classify(&ct("application/vnd.api+json; charset=utf-8"), b"[1]"),
        BodyKind::Json
    );
    assert_eq!(classify(&[], br#" {"sniffed": true} "#), BodyKind::Json);
    assert_eq!(classify(&ct("text/plain"), b"[1, 2]"), BodyKind::Json);
    assert_eq!(
        classify(&ct("text/plain"), b"{not json}"),
        BodyKind::Text(None)
    );
    assert_eq!(
        classify(&ct("text/html"), b"<p>"),
        BodyKind::Text(Some("html"))
    );
    assert_eq!(
        classify(&ct("application/atom+xml"), b"<a/>"),
        BodyKind::Text(Some("xml"))
    );
    assert_eq!(classify(&ct("image/png"), b"PNG"), BodyKind::Binary);
    assert_eq!(
        classify(&ct("text/plain"), &[0xff, 0xfe, 0]),
        BodyKind::Binary
    );
    assert_eq!(classify(&ct("application/json"), b""), BodyKind::Empty);
    assert_eq!(
        classify(&ct("application/octet-stream"), b"abc"),
        BodyKind::Binary
    );
}

#[test]
fn sizes_dumps_and_truncation() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1023), "1023 B");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    assert_eq!(truncate("héllo", 2), ("h", true));
    assert_eq!(truncate("abc", 5), ("abc", false));
    let dump = hex_dump(b"ABCDEFGHIJKLMNOPQ\x00", 17);
    assert_eq!(
        dump,
        "00000000  41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 50  ABCDEFGHIJKLMNOP\n\
         00000010  51                                               Q\n\
         ... 1 more bytes\n"
    );
}

#[test]
fn raw_text_shows_the_wire_format() {
    let r = http::Response {
        status: 404,
        reason: "Not Found".into(),
        version: "HTTP/1.1".into(),
        final_url: "http://x".into(),
        remote_addr: None,
        headers: ct("text/plain"),
        body: b"nope".to_vec(),
        truncated: true,
        timing: http::Timing {
            headers_ms: 1,
            total_ms: 2,
        },
    };
    assert_eq!(
        raw_text(&r, 3),
        "HTTP/1.1 404 Not Found\nContent-Type: text/plain\n\nnop\n\n[... truncated]"
    );
}
