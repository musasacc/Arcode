//! Adversarial review findings against JetBrains HTTP Client samples
//! (failing on purpose until fixed).

use std::collections::BTreeMap;

use crate::exchange::{Context, Settings, build};
use crate::parse::{BodyPart, parse};
use crate::test_support::TempDir;
use crate::vars::SystemDynamic;

/// JetBrains "Redirect output to a custom file or directory":
/// `>> path` / `>>! path` after a request saves the response. They are
/// not body text; today they are sent as the GET's body.
#[test]
fn response_output_redirects_are_not_body_text() {
    let src = "\
GET https://example.com/get

>> ./myFolder/myFile.json

###
POST https://example.com/post
Content-Type: application/json

{\"a\": 1}

>>! ./myFile.json
";
    let file = parse(src);
    assert_eq!(file.requests.len(), 2);
    assert!(
        file.requests[0].body.is_empty(),
        "GET body: {:?}",
        file.requests[0].body
    );
    assert_eq!(
        file.requests[1].body,
        [BodyPart::Text("{\"a\": 1}".into())],
        "POST body"
    );
}

/// JetBrains pre-request scripts (`< {% ... %}` before the request line)
/// cannot run (no JS engine), but must not become the request line: today
/// `< {%` is parsed as `GET < {%` and the real request is lost.
#[test]
fn pre_request_scripts_are_skipped() {
    let src = "\
< {%
    request.variables.set(\"firstname\", \"John\")
%}
GET http://example.org/hello
Accept: text/plain
";
    let file = parse(src);
    assert_eq!(file.requests.len(), 1);
    let r = &file.requests[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str(), r.line),
        ("GET", "http://example.org/hello", 3)
    );
    assert_eq!(r.headers, [("Accept".into(), "text/plain".into())]);
    assert!(r.body.is_empty(), "{:?}", r.body);
}

/// The JetBrains multipart sample. Multipart delimiters must be preceded
/// by CRLF (RFC 2046 §5.1.1; the JetBrains client sends CRLF), but body
/// lines are joined with a bare `\n`, so strict servers see one part.
#[test]
fn multipart_bodies_use_crlf_delimiters() {
    let dir = TempDir::new("multipart");
    std::fs::write(dir.path().join("data.json"), "{\"k\": 1}").expect("write");
    let src = "\
POST https://example.com/upload
Content-Type: multipart/form-data; boundary=WebAppBoundary

--WebAppBoundary
Content-Disposition: form-data; name=\"field-name\"

field-value
--WebAppBoundary
Content-Disposition: form-data; name=\"data\"; filename=\"data.json\"
Content-Type: application/json

< ./data.json
--WebAppBoundary--
";
    let vars = BTreeMap::new();
    let ctx = Context {
        vars: &vars,
        project: dir.path(),
        doc_path: Some("a.http"),
        settings: Settings::default(),
    };
    let file = parse(src);
    let body = build(&file.requests[0], &ctx, &mut SystemDynamic::default())
        .expect("build")
        .request
        .body;
    let body = String::from_utf8(body).expect("utf-8");
    assert!(
        body.contains("field-value\r\n--WebAppBoundary\r\n"),
        "delimiters need CRLF: {body:?}"
    );
    assert!(
        body.contains("\r\n\r\n{\"k\": 1}\r\n--WebAppBoundary--"),
        "{body:?}"
    );
}
