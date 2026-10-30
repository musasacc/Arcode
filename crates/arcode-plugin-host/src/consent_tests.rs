//! Tests for prompts on top of the grant state machine.

use super::*;
use crate::manifest::PluginSource;

fn manifest(capabilities: &str) -> Manifest {
    let text = format!(
        "[plugin]\nid = \"test.consent\"\nname = \"Consent Test\"\nversion = \"0.1.0\"\napi = \"0.1\"\n\
         [capabilities]\n{capabilities}\n"
    );
    Manifest::parse(&text, PluginSource::Dev).unwrap()
}

fn perms(answers: impl IntoIterator<Item = ConsentAnswer>) -> (Permissions, Arc<ScriptedConsent>) {
    let provider = Arc::new(ScriptedConsent::new(answers));
    (Permissions::new(GrantStore::in_memory(true), provider.clone()), provider)
}

fn allow(scope: GrantScope, write: bool) -> ConsentAnswer {
    ConsentAnswer::Allow { scope, write }
}

#[test]
fn workspace_prompt_is_asked_once_per_scope() {
    let m = manifest("workspace = \"read\"\nnetwork = { hosts = [\"*\"] }");
    let (p, asked) = perms([allow(GrantScope::Project, true)]);
    let token = CancelToken::new();
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::Read);
    // Remembered for the project: no second prompt.
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::Read);
    let asked = asked.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].kind,
        ConsentKind::Workspace {
            write: false,
            network: true
        }
    );
    assert_eq!(
        asked[0].message(),
        "\u{201c}Consent Test\u{201d} can read this project and send data to hosts you approve."
    );
}

#[test]
fn write_checkbox_may_be_left_off() {
    let m = manifest("workspace = \"read-write\"");
    let token = CancelToken::new();
    let (p, _) = perms([allow(GrantScope::Global, false)]);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::Read);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::Read);

    let (p, asked) = perms([allow(GrantScope::Once, true), allow(GrantScope::Once, true)]);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::ReadWrite);
    // "Once" covers one instance only: asked again.
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::ReadWrite);
    assert_eq!(asked.asked().len(), 2);
}

#[test]
fn deny_and_dismiss() {
    let m = manifest("workspace = \"read\"");
    let token = CancelToken::new();
    let (p, asked) = perms([ConsentAnswer::Deny]);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::None);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::None);
    assert_eq!(asked.asked().len(), 1, "a denial is remembered");

    let (p, asked) = perms([ConsentAnswer::Dismissed]);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::None);
    assert_eq!(p.file_grant(&m, "proj", true, &token), FileGrant::None);
    assert_eq!(asked.asked().len(), 2, "a dismissal asks again");
}

#[test]
fn no_workspace_capability_never_prompts() {
    let m = manifest("editor = \"context\"");
    let (p, asked) = perms([]);
    assert_eq!(p.file_grant(&m, "proj", true, &CancelToken::new()), FileGrant::None);
    assert!(asked.asked().is_empty());
}

#[test]
fn network_check_per_host_and_master_switch() {
    let m = manifest("network = { hosts = [\"*\"] }");
    let token = CancelToken::new();
    let (p, asked) = perms([allow(GrantScope::Global, false), ConsentAnswer::Deny]);
    let kind = |host: &str| ConsentKind::Network {
        method: "GET".into(),
        scheme: "https".into(),
        host: host.into(),
        port: 443,
    };
    let a = Capability::Network { host: "a.test".into() };
    let b = Capability::Network { host: "b.test".into() };
    assert!(p.check(&m, None, &a, kind("a.test"), true, &token));
    assert!(p.check(&m, None, &a, kind("a.test"), true, &token));
    assert!(!p.check(&m, None, &b, kind("b.test"), true, &token));
    assert!(!p.check(&m, None, &b, kind("b.test"), true, &token));
    assert_eq!(asked.asked().len(), 2);
    // Master switch off: denied without a prompt even though granted.
    assert!(!p.check(&m, None, &a, kind("a.test"), false, &token));
    assert_eq!(asked.asked().len(), 2);
    assert_eq!(
        asked.asked()[0].message(),
        "\u{201c}Consent Test\u{201d} wants to send a GET request to https://a.test:443."
    );
}

#[test]
fn quoting_caps_and_neutralises_plugin_text() {
    assert_eq!(quote("a\u{1b}[31mb"), "\u{201c}a [31mb\u{201d}");
    let long = "x".repeat(200);
    let q = quote(&long);
    assert_eq!(q.chars().count(), MAX_QUOTED_CHARS + 3);
    assert!(q.ends_with("…\u{201d}"));
}
