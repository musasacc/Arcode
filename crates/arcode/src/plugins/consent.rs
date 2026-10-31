//! Consent prompts, interim version (docs/PLUGINS.md §4): a host-drawn
//! prompt (the app's themed in-window dialog) built only from the
//! host-derived [`ConsentRequest`], plus a scripted answer for automated
//! runs. The full consent dialog (write checkbox, capability summary) is a
//! separate step; it replaces [`buttons`] and [`answer_for`].
//!
//! `ARCODE_PLUGIN_CONSENT` answers every prompt without showing it:
//! `allow-once`, `allow-project`, `allow-always` (each optionally with
//! `+write` for workspace prompts) or `deny`. It is read from the
//! environment of the Arcode process only, never from a project, and only
//! by debug builds: a release build always asks.

use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ConsentRequest};
use arcode_plugin_host::grants::GrantScope;

/// The environment variable with a scripted answer.
pub const SCRIPTED_ENV: &str = "ARCODE_PLUGIN_CONSENT";

/// Parses a scripted answer (see the module docs).
pub fn parse_scripted(text: &str) -> Option<ConsentAnswer> {
    let text = text.trim();
    let (scope, write) = match text.strip_suffix("+write") {
        Some(scope) => (scope, true),
        None => (text, false),
    };
    let scope = match scope {
        "deny" if !write => return Some(ConsentAnswer::Deny),
        "allow-once" => GrantScope::Once,
        "allow-project" => GrantScope::Project,
        "allow-always" => GrantScope::Global,
        _ => return None,
    };
    Some(ConsentAnswer::Allow { scope, write })
}

/// The scripted answer from the environment, if set and valid (debug
/// builds only).
pub fn scripted() -> Option<ConsentAnswer> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let value = std::env::var(SCRIPTED_ENV).ok()?;
    let answer = parse_scripted(&value);
    if answer.is_none() {
        log::warn!("{SCRIPTED_ENV}={value:?} is not a known answer; prompting instead");
    }
    answer
}

/// The prompt's buttons for `kind`, first is the default; the last one
/// (Deny) is what Escape picks.
pub fn buttons(kind: &ConsentKind) -> &'static [&'static str] {
    match kind {
        ConsentKind::Network { .. } => &["Allow Once", "This Project", "Always", "Deny"],
        ConsentKind::Workspace { write: true, .. } => &[
            "Allow Reading",
            "Allow Reading and Writing",
            "Allow Once (Read)",
            "Deny",
        ],
        ConsentKind::Workspace { write: false, .. } | ConsentKind::EditorWrite => {
            &["Allow for This Project", "Allow Once", "Deny"]
        }
    }
}

/// The answer for the button at `index` of [`buttons`]; out of range is
/// a dismissal.
pub fn answer_for(kind: &ConsentKind, index: usize) -> ConsentAnswer {
    let allow = |scope, write| ConsentAnswer::Allow { scope, write };
    match (kind, index) {
        (ConsentKind::Network { .. }, 0) => allow(GrantScope::Once, false),
        (ConsentKind::Network { .. }, 1) => allow(GrantScope::Project, false),
        (ConsentKind::Network { .. }, 2) => allow(GrantScope::Global, false),
        (ConsentKind::Network { .. }, 3) => ConsentAnswer::Deny,
        (ConsentKind::Workspace { write: true, .. }, 0) => allow(GrantScope::Project, false),
        (ConsentKind::Workspace { write: true, .. }, 1) => allow(GrantScope::Project, true),
        (ConsentKind::Workspace { write: true, .. }, 2) => allow(GrantScope::Once, false),
        (ConsentKind::Workspace { write: true, .. }, 3) => ConsentAnswer::Deny,
        (_, 0) => allow(GrantScope::Project, false),
        (_, 1) => allow(GrantScope::Once, false),
        (_, 2) => ConsentAnswer::Deny,
        _ => ConsentAnswer::Dismissed,
    }
}

/// The prompt's detail line: what the plugin may do afterwards.
pub fn detail(request: &ConsentRequest) -> String {
    match &request.kind {
        ConsentKind::Network { .. } => {
            "Requests are only sent when you click; you are asked once per host.".to_owned()
        }
        ConsentKind::Workspace { .. } => match &request.project {
            Some(project) => format!("Project: {project}. Nothing outside it is reachable."),
            None => "Nothing outside the project is reachable.".to_owned(),
        },
        ConsentKind::EditorWrite => "Each change is one undo step.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_answers() {
        assert_eq!(parse_scripted("deny"), Some(ConsentAnswer::Deny));
        assert_eq!(
            parse_scripted(" allow-once "),
            Some(ConsentAnswer::Allow {
                scope: GrantScope::Once,
                write: false
            })
        );
        assert_eq!(
            parse_scripted("allow-project+write"),
            Some(ConsentAnswer::Allow {
                scope: GrantScope::Project,
                write: true
            })
        );
        assert_eq!(
            parse_scripted("allow-always"),
            Some(ConsentAnswer::Allow {
                scope: GrantScope::Global,
                write: false
            })
        );
        assert_eq!(parse_scripted("deny+write"), None);
        assert_eq!(parse_scripted("yes"), None);
    }

    #[test]
    fn every_button_maps_to_an_answer() {
        let kinds = [
            ConsentKind::Network {
                method: "GET".into(),
                scheme: "http".into(),
                host: "127.0.0.1".into(),
                port: 80,
            },
            ConsentKind::Workspace {
                write: true,
                network: false,
            },
            ConsentKind::Workspace {
                write: false,
                network: true,
            },
            ConsentKind::EditorWrite,
        ];
        for kind in &kinds {
            let labels = buttons(kind);
            for (i, label) in labels.iter().enumerate() {
                let answer = answer_for(kind, i);
                assert_ne!(answer, ConsentAnswer::Dismissed, "{label}");
                assert_eq!(*label == "Deny", answer == ConsentAnswer::Deny);
            }
            assert_eq!(answer_for(kind, labels.len()), ConsentAnswer::Dismissed);
        }
        assert_eq!(
            answer_for(&kinds[1], 1),
            ConsentAnswer::Allow {
                scope: GrantScope::Project,
                write: true
            }
        );
    }
}
