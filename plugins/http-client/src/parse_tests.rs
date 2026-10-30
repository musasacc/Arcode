//! `parse` tests, mostly with samples in the JetBrains HTTP Client and VS
//! Code REST Client documentation style.

use super::*;

fn text(s: &str) -> BodyPart {
    BodyPart::Text(s.to_owned())
}

#[test]
fn jetbrains_sample_file() {
    let src = "\
### GET request with a header
GET https://httpbin.org/ip
Accept: application/json

### GET request with parameter
GET https://httpbin.org/get?show_env=1
Accept: application/json

### POST request with a json body
POST https://httpbin.org/post
Content-Type: application/json

{
  \"id\": 999,
  \"value\": \"content\"
}

";
    let file = parse(src);
    assert_eq!(file.requests.len(), 3);
    let r = &file.requests[0];
    assert_eq!(r.name.as_deref(), Some("GET request with a header"));
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("GET", "https://httpbin.org/ip")
    );
    assert_eq!(r.headers, [("Accept".into(), "application/json".into())]);
    assert!(r.body.is_empty());
    assert_eq!(r.line, 1);
    assert_eq!(r.span, (0, 4));
    let post = &file.requests[2];
    assert_eq!(post.line, 9);
    assert_eq!(
        post.body,
        [text("{\n  \"id\": 999,\n  \"value\": \"content\"\n}")]
    );
}

#[test]
fn bare_url_is_get_and_version_is_split_off() {
    let file = parse("https://example.com/a\n###\nDELETE http://x/y HTTP/1.1\n");
    assert_eq!(file.requests[0].method, "GET");
    assert_eq!(file.requests[0].url, "https://example.com/a");
    assert_eq!(file.requests[0].version, None);
    assert_eq!(file.requests[1].method, "DELETE");
    assert_eq!(file.requests[1].version.as_deref(), Some("HTTP/1.1"));
    assert_eq!(file.requests[1].url, "http://x/y");
}

#[test]
fn comments_names_and_directives() {
    let src = "\
# a plain comment
// another one
### Separator name
# @name login
// @no-redirect
# @timeout 5
POST {{host}}/login
# comment between headers
Content-Type: application/x-www-form-urlencoded
// and another

user=a&pass=b
# not a comment inside the body
";
    let file = parse(src);
    let r = &file.requests[0];
    assert_eq!(r.name.as_deref(), Some("login"), "@name wins over ###");
    assert!(r.no_redirect);
    assert_eq!(r.timeout_ms, Some(5000));
    assert_eq!(r.headers.len(), 1);
    assert_eq!(
        r.body,
        [text("user=a&pass=b\n# not a comment inside the body")]
    );
    // The text before the first separator has no request line.
    assert_eq!(file.requests.len(), 1);
    assert_eq!(
        parse("# @name=x\nGET /a").requests[0].name.as_deref(),
        Some("x")
    );
}

#[test]
fn file_variables_are_collected_in_order() {
    let src = "\
@host = localhost:8080
@token=abc
@empty =
GET http://{{host}}/x
Authorization: Bearer {{token}}

###
@host = example.com
GET http://{{host}}/y
";
    let file = parse(src);
    assert_eq!(
        file.variables,
        [
            ("host".into(), "localhost:8080".into()),
            ("token".into(), "abc".into()),
            ("empty".into(), String::new()),
            ("host".into(), "example.com".into()),
        ]
    );
    assert_eq!(file.requests.len(), 2);
    assert_eq!(file.requests[0].url, "http://{{host}}/x");
}

#[test]
fn indented_lines_continue_the_url() {
    let src = "GET http://example.com/api\n    /users\n    ?page=1\n    &size=20\nAccept: */*\n";
    let r = &parse(src).requests[0];
    assert_eq!(r.url, "http://example.com/api/users?page=1&size=20");
    assert_eq!(r.headers.len(), 1);
}

#[test]
fn file_includes_and_handlers() {
    let src = "\
POST http://x/upload
Content-Type: application/json

< ./body.json
<root/>
<> 2024-01-01T000000.200.json
> {%
    client.global.set(\"token\", response.body.token);
%}
> ./check.js
> {% client.log(1) %}
tail
";
    let r = &parse(src).requests[0];
    assert_eq!(
        r.body,
        [BodyPart::File("./body.json".into()), text("<root/>\ntail"),]
    );
}

#[test]
fn crlf_and_trailing_blank_lines() {
    let src = "POST http://x\r\nA: 1\r\n\r\nline1\r\nline2\r\n\r\n\r\n###\r\nGET http://y\r\n";
    let file = parse(src);
    assert_eq!(file.requests[0].body, [text("line1\nline2")]);
    assert_eq!(file.requests[0].headers, [("A".into(), "1".into())]);
    assert_eq!(file.requests[1].url, "http://y");
}

#[test]
fn body_without_blank_line_is_lenient() {
    let r = &parse("POST http://x\nContent-Type: text/plain\n{\"a\": 1}\n").requests[0];
    assert_eq!(r.headers.len(), 1);
    assert_eq!(r.body, [text("{\"a\": 1}")]);
}

#[test]
fn request_at_finds_the_block_of_a_line() {
    let src = "GET http://a\n\n###\n# c\nGET http://b\nX: y\n\nbody\n";
    let file = parse(src);
    assert_eq!(file.request_at(0).map(|r| r.url.as_str()), Some("http://a"));
    assert_eq!(file.request_at(1).map(|r| r.url.as_str()), Some("http://a"));
    for line in 2..=7 {
        assert_eq!(
            file.request_at(line).map(|r| r.url.as_str()),
            Some("http://b"),
            "line {line}"
        );
    }
    assert!(file.request_at(99).is_none());
}

#[test]
fn empty_and_comment_only_files_have_no_requests() {
    assert!(parse("").requests.is_empty());
    assert!(parse("###\n# nothing\n@a = 1\n###\n").requests.is_empty());
}

#[test]
fn durations() {
    assert_eq!(parse_duration_ms("5"), Some(5000));
    assert_eq!(parse_duration_ms("250 ms"), Some(250));
    assert_eq!(parse_duration_ms("2m"), Some(120_000));
    assert_eq!(parse_duration_ms("x"), None);
    assert_eq!(parse_duration_ms("9999999999"), None);
}

#[test]
fn custom_methods_and_spaces_in_header_values() {
    let r = &parse("PROPFIND http://dav/x\nDepth:   1  \n").requests[0];
    assert_eq!(r.method, "PROPFIND");
    assert_eq!(r.headers, [("Depth".into(), "1".into())]);
    // Standard methods are accepted in any case.
    let r = &parse("post http://x").requests[0];
    assert_eq!((r.method.as_str(), r.url.as_str()), ("POST", "http://x"));
}
