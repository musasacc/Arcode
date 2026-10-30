//! Exhaustive tests of the grant state machine (§13 step 5).

use super::*;
use crate::manifest::NetworkCapability;

fn plugin() -> PluginId {
    PluginId::parse("test.plugin").unwrap()
}

fn other() -> PluginId {
    PluginId::parse("test.other").unwrap()
}

fn everything() -> Capabilities {
    Capabilities {
        workspace: WorkspaceAccess::ReadWrite,
        editor: EditorAccess::AllOpen,
        editor_write: true,
        network: Some(NetworkCapability {
            hosts: vec!["*".into()],
            background: true,
        }),
    }
}

fn all_caps() -> Vec<Capability> {
    vec![
        Capability::WorkspaceRead,
        Capability::WorkspaceWrite,
        Capability::EditorAllOpen,
        Capability::EditorWrite,
        Capability::Network {
            host: "example.com".into(),
        },
        Capability::NetworkBackground,
    ]
}

fn with_project() -> Grants {
    Grants {
        has_project: true,
        ..Grants::default()
    }
}

#[test]
fn defaults_ask_except_background_network() {
    let g = with_project();
    for cap in all_caps() {
        let expected = if cap == Capability::NetworkBackground {
            Decision::Deny
        } else {
            Decision::Ask
        };
        assert_eq!(
            g.decide(&plugin(), &cap, &everything(), true),
            expected,
            "{cap}"
        );
    }
}

#[test]
fn undeclared_capabilities_are_denied_even_if_granted() {
    let mut g = with_project();
    for cap in all_caps() {
        g.record(&plugin(), &cap, GrantState::Allow, GrantScope::Global);
    }
    let none = Capabilities::default();
    for cap in all_caps() {
        assert_eq!(
            g.decide(&plugin(), &cap, &none, true),
            Decision::Deny,
            "{cap}"
        );
    }
    let read_only = Capabilities {
        workspace: WorkspaceAccess::Read,
        editor: EditorAccess::Context,
        network: Some(NetworkCapability {
            hosts: vec!["*".into()],
            background: false,
        }),
        ..Capabilities::default()
    };
    let decide = |c: Capability| g.decide(&plugin(), &c, &read_only, true);
    assert_eq!(decide(Capability::WorkspaceRead), Decision::Allow);
    assert_eq!(decide(Capability::WorkspaceWrite), Decision::Deny);
    assert_eq!(decide(Capability::EditorAllOpen), Decision::Deny);
    assert_eq!(decide(Capability::EditorWrite), Decision::Deny);
    assert_eq!(decide(Capability::NetworkBackground), Decision::Deny);
}

/// Every capability × state × scope, with and without a project.
#[test]
fn record_then_decide_for_every_combination() {
    for has_project in [true, false] {
        for cap in all_caps() {
            for state in [GrantState::Allow, GrantState::Deny] {
                for scope in [GrantScope::Once, GrantScope::Project, GrantScope::Global] {
                    let mut g = Grants {
                        has_project,
                        ..Grants::default()
                    };
                    let before = g.decide(&plugin(), &cap, &everything(), true);
                    let answered = g.record(&plugin(), &cap, state, scope);
                    assert_eq!(answered, Decision::from(state));
                    let after = g.decide(&plugin(), &cap, &everything(), true);
                    let ctx = format!("{cap} {state:?} {scope:?} project={has_project}");
                    match scope {
                        GrantScope::Once => assert_eq!(after, before, "{ctx}: once is not stored"),
                        _ => assert_eq!(after, Decision::from(state), "{ctx}"),
                    }
                    // Stored where the scope says.
                    let key = (plugin(), cap.clone());
                    let (in_session, in_project, in_global) = (
                        g.session.contains_key(&key),
                        g.project.contains_key(&key),
                        g.global.contains_key(&key),
                    );
                    let expected = match scope {
                        GrantScope::Once => (false, false, false),
                        GrantScope::Project if has_project => (false, true, false),
                        GrantScope::Project => (true, false, false),
                        GrantScope::Global => (false, false, true),
                    };
                    assert_eq!((in_session, in_project, in_global), expected, "{ctx}");
                    // Never leaks to another plugin.
                    assert_eq!(
                        g.decide(&other(), &cap, &everything(), true),
                        before,
                        "{ctx}"
                    );
                    // Master switch: network off denies network caps only.
                    let switched = g.decide(&plugin(), &cap, &everything(), false);
                    if matches!(
                        cap,
                        Capability::Network { .. } | Capability::NetworkBackground
                    ) {
                        assert_eq!(switched, Decision::Deny, "{ctx}");
                    } else {
                        assert_eq!(switched, after, "{ctx}");
                    }
                    // Revoke returns to the default.
                    assert_eq!(
                        g.revoke(&plugin(), &cap),
                        scope != GrantScope::Once,
                        "{ctx}"
                    );
                    assert_eq!(
                        g.decide(&plugin(), &cap, &everything(), true),
                        before,
                        "{ctx}"
                    );
                }
            }
        }
    }
}

#[test]
fn narrower_scopes_win_and_global_answers_replace_them() {
    let mut g = with_project();
    let net = Capability::Network {
        host: "api.example.com".into(),
    };
    g.record(&plugin(), &net, GrantState::Deny, GrantScope::Global);
    g.record(&plugin(), &net, GrantState::Allow, GrantScope::Project);
    assert_eq!(
        g.decide(&plugin(), &net, &everything(), true),
        Decision::Allow
    );
    // "Always" after a project answer overrides it.
    g.record(&plugin(), &net, GrantState::Deny, GrantScope::Global);
    assert_eq!(
        g.decide(&plugin(), &net, &everything(), true),
        Decision::Deny
    );
    assert!(g.project.is_empty());
    // Session (no-project) answers beat project and global ones.
    g.session.insert((plugin(), net.clone()), GrantState::Allow);
    assert_eq!(
        g.decide(&plugin(), &net, &everything(), true),
        Decision::Allow
    );
}

#[test]
fn hosts_are_separate_grants() {
    let mut g = with_project();
    let a = Capability::Network {
        host: "a.test".into(),
    };
    let b = Capability::Network {
        host: "b.test".into(),
    };
    g.record(&plugin(), &a, GrantState::Allow, GrantScope::Global);
    assert_eq!(
        g.decide(&plugin(), &a, &everything(), true),
        Decision::Allow
    );
    assert_eq!(g.decide(&plugin(), &b, &everything(), true), Decision::Ask);
}

#[test]
fn listing_and_background_indicator() {
    let mut g = with_project();
    assert!(!g.any_background_network());
    g.record(
        &plugin(),
        &Capability::NetworkBackground,
        GrantState::Deny,
        GrantScope::Global,
    );
    assert!(!g.any_background_network());
    g.record(
        &plugin(),
        &Capability::NetworkBackground,
        GrantState::Allow,
        GrantScope::Project,
    );
    g.record(
        &plugin(),
        &Capability::WorkspaceRead,
        GrantState::Allow,
        GrantScope::Global,
    );
    g.record(
        &other(),
        &Capability::WorkspaceRead,
        GrantState::Deny,
        GrantScope::Global,
    );
    assert!(g.any_background_network());
    let mut listed = g.list(&plugin());
    listed.sort();
    assert_eq!(
        listed,
        [
            (
                Capability::WorkspaceRead,
                GrantState::Allow,
                GrantScope::Global
            ),
            (
                Capability::NetworkBackground,
                GrantState::Allow,
                GrantScope::Project
            ),
            (
                Capability::NetworkBackground,
                GrantState::Deny,
                GrantScope::Global
            ),
        ]
    );
    assert!(Capability::WorkspaceWrite.is_static());
    assert!(!Capability::EditorWrite.is_static());
}
