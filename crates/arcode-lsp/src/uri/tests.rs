//! Unit tests for path <-> URI conversion.

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
fn windows_unc_hosts_are_percent_encoded() {
    assert_eq!(
        path_str_to_uri_string(r"\\files.example.com@SSL@443\DavWWWRoot\a.rs", true),
        "file://files.example.com%40ssl%40443/DavWWWRoot/a.rs"
    );
    assert_eq!(
        path_str_to_uri_string("\\\\B\u{fc}cher-PC\\share\\a.rs", true),
        "file://b%C3%BCcher-pc/share/a.rs"
    );
    assert_eq!(path_str_to_uri_string(r"\\h:1\s", true), "file://h%3A1/s");
    for p in [
        r"\\files.example.com@SSL@443\DavWWWRoot\a.rs",
        "\\\\B\u{fc}cher-PC\\share\\a.rs",
        r"\\a b\c",
        r"\\[fe80::1]\c",
    ] {
        let uri = path_str_to_uri_string(p, true);
        assert!(Uri::from_str(&uri).is_ok(), "{p:?} -> {uri}");
        // Server names are case-insensitive and come back lowercased.
        assert_eq!(
            uri_str_to_path_string(&uri, true),
            Some(p.to_lowercase().replace("davwwwroot", "DavWWWRoot")),
            "{uri}"
        );
    }
}

#[test]
fn every_windows_path_string_gives_a_valid_uri() {
    for p in [
        r"C:\a%b\#?[]{}^`|<>\x",
        "C:\\\u{1F600}\\\u{0}",
        r"\\?\UNC\h@x\s",
        r"\\",
        r"\\host",
        r"relative\path",
        "",
    ] {
        let uri = path_str_to_uri_string(p, true);
        assert!(Uri::from_str(&uri).is_ok(), "{p:?} -> {uri}");
    }
}

#[test]
fn strict_fallback_is_valid() {
    let s = format!("file:///{}", encode_strict(r"\\h@x\a b\\u{e9}"));
    assert!(Uri::from_str(&s).is_ok(), "{s}");
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
