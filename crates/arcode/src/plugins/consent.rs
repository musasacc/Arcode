//! Consent (docs/PLUGINS.md §4): [`AppConsent`], the app's
//! [`ConsentProvider`], and [`DialogModel`], what the consent dialog
//! (`consent_dialog`) shows.
//!
//! The host calls the provider on the plugin's thread. It hands the
//! request to the workspace as a [`ConsentAsk`] and blocks that thread
//! until the user answers, the call is cancelled or the dialog goes away
//! (dismissed: denied this time, asked again next time). The UI thread
//! never waits on a plugin. Network prompts only happen inside a user
//! gesture: the host refuses `http.send` before asking otherwise.
//!
//! What the user decides on comes from the host, never from plugin text:
//! the capability (the request kind), the method, scheme, host and port the
//! host parsed, the validated plugin id and where the plugin was found
//! (bundled, installed, unpacked). The plugin's display name and the
//! project name are shown as the host quoted them, on a line of their own,
//! so a name cannot read as part of the question.

use arcode_plugin_host::PluginSource;
use arcode_plugin_host::cancel::{CancelToken, ReplySender, oneshot};
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ConsentProvider, ConsentRequest};
use arcode_plugin_host::grants::GrantScope;

/// A prompt waiting for the user, with where the answer goes.
#[derive(Debug)]
pub struct ConsentAsk {
    /// What is asked (host-derived).
    pub request: ConsentRequest,
    /// The call's token: once it fires nobody waits for the answer.
    pub cancel: CancelToken,
    /// The waiting plugin thread; dropping it unanswered dismisses.
    pub reply: ReplySender<ConsentAnswer>,
}

/// The [`ConsentProvider`] the app gives the host: prompts go to the
/// workspace, which shows them one at a time.
#[derive(Debug)]
pub struct AppConsent {
    asks: async_channel::Sender<ConsentAsk>,
}

impl AppConsent {
    /// A provider and the receiver the workspace drains.
    pub fn new() -> (Self, async_channel::Receiver<ConsentAsk>) {
        let (asks, rx) = async_channel::unbounded();
        (Self { asks }, rx)
    }
}

impl ConsentProvider for AppConsent {
    fn ask(&self, request: &ConsentRequest, cancel: &CancelToken) -> ConsentAnswer {
        if cancel.is_cancelled() {
            return ConsentAnswer::Dismissed;
        }
        let (reply, answer) = oneshot();
        let ask = ConsentAsk {
            request: request.clone(),
            cancel: cancel.clone(),
            reply,
        };
        if self.asks.try_send(ask).is_err() {
            // No window to ask in.
            return ConsentAnswer::Dismissed;
        }
        answer
            .wait(cancel, None)
            .unwrap_or(ConsentAnswer::Dismissed)
    }
}

/// An answer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// This request (or this instance, for file access) only.
    Once,
    /// Remembered for the open project (the session without one).
    Project,
    /// Remembered for every project.
    Always,
    /// Refused, and not asked again in this project.
    Deny,
}

impl Choice {
    /// The buttons in display order.
    pub const ALL: [Choice; 4] = [Self::Once, Self::Project, Self::Always, Self::Deny];

    /// The button label.
    pub fn label(self, has_project: bool) -> &'static str {
        match self {
            Self::Once => "Allow once",
            Self::Project if has_project => "Always for this project",
            Self::Project => "For this session",
            Self::Always => "Always",
            Self::Deny => "Deny",
        }
    }

    /// The answer this button gives; `write` is the workspace prompt's
    /// write checkbox (ignored by other prompts).
    pub fn answer(self, write: bool) -> ConsentAnswer {
        let allow = |scope| ConsentAnswer::Allow { scope, write };
        match self {
            Self::Once => allow(GrantScope::Once),
            Self::Project => allow(GrantScope::Project),
            Self::Always => allow(GrantScope::Global),
            Self::Deny => ConsentAnswer::Deny,
        }
    }
}

/// How an answer reads in the plugin log.
pub fn describe(answer: ConsentAnswer) -> &'static str {
    match answer {
        ConsentAnswer::Allow { scope, write } => match (scope, write) {
            (GrantScope::Once, false) => "allowed once",
            (GrantScope::Once, true) => "allowed once (with writes)",
            (GrantScope::Project, false) => "allowed for the project",
            (GrantScope::Project, true) => "allowed for the project (with writes)",
            (GrantScope::Global, false) => "always allowed",
            (GrantScope::Global, true) => "always allowed (with writes)",
        },
        ConsentAnswer::Deny => "denied",
        ConsentAnswer::Dismissed => "dismissed",
    }
}

/// Where a plugin was found, as the dialog and the Plugins view say it.
pub fn source_label(source: Option<PluginSource>) -> &'static str {
    match source {
        Some(PluginSource::Bundled) => "bundled with Arcode",
        Some(PluginSource::Installed) => "installed by you",
        Some(PluginSource::Dev) => "loaded unpacked (development)",
        None => "unknown plugin",
    }
}

/// What the consent dialog shows for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogModel {
    /// The question (host wording).
    pub title: &'static str,
    /// The plugin's display name, quoted by the host.
    pub plugin_name: String,
    /// The validated plugin id.
    pub plugin_id: String,
    /// Where the plugin was found ([`source_label`]).
    pub source: &'static str,
    /// What it asks to do, before [`DialogModel::target`].
    pub action: String,
    /// What the answer is about, shown in monospace: the origin of a
    /// request, or the quoted project.
    pub target: String,
    /// Further host wording (what the answer covers).
    pub notes: Vec<String>,
    /// The workspace prompt's "also allow changing files" checkbox.
    pub write_option: bool,
    /// A project is open ("this project" scope; else the session).
    pub has_project: bool,
}

/// `scheme://host:port`, with an IPv6 literal in brackets.
pub fn origin(scheme: &str, host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("{scheme}://[{host}]:{port}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

/// The dialog for `request` from a plugin found at `source`.
pub fn model(request: &ConsentRequest, source: Option<PluginSource>) -> DialogModel {
    let project = request
        .project
        .clone()
        .unwrap_or_else(|| "(no project open)".to_owned());
    let (title, action, target, notes, write_option) = match &request.kind {
        ConsentKind::Network {
            method,
            scheme,
            host,
            port,
        } => (
            "Allow a network request?",
            format!("Send a {method} request to"),
            origin(scheme, host, *port),
            vec![
                format!("Your answer covers every request to {host}."),
                "Requests are only sent when you click; you are asked once per host.".to_owned(),
            ],
            false,
        ),
        ConsentKind::Workspace { write, network } => {
            let mut notes = vec!["Nothing outside the project is reachable.".to_owned()];
            if *network {
                notes.push(
                    "It can also send data to hosts you approve (each one is asked for).".into(),
                );
            }
            (
                "Allow access to project files?",
                "Read the files of project".to_owned(),
                project,
                notes,
                *write,
            )
        }
        ConsentKind::EditorWrite => (
            "Allow editing documents?",
            "Edit open documents of project".to_owned(),
            project,
            vec!["Each change is one undo step.".to_owned()],
            false,
        ),
    };
    DialogModel {
        title,
        plugin_name: request.plugin_name.clone(),
        plugin_id: request.plugin.to_string(),
        source: source_label(source),
        action,
        target,
        notes,
        write_option,
        has_project: request.project.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arcode_plugin_host::PluginId;
    use arcode_plugin_host::consent::quote;

    use super::*;

    fn request(kind: ConsentKind, project: Option<&str>) -> ConsentRequest {
        ConsentRequest {
            plugin: PluginId::parse("acme.http").unwrap(),
            plugin_name: quote("Acme HTTP"),
            project: project.map(quote),
            kind,
        }
    }

    fn network(host: &str) -> ConsentKind {
        ConsentKind::Network {
            method: "POST".into(),
            scheme: "https".into(),
            host: host.into(),
            port: 8443,
        }
    }

    #[test]
    fn network_prompts_show_the_parsed_origin() {
        let m = model(
            &request(network("api.example.com"), Some("shop")),
            Some(PluginSource::Installed),
        );
        assert_eq!(m.title, "Allow a network request?");
        assert_eq!(m.action, "Send a POST request to");
        assert_eq!(m.target, "https://api.example.com:8443");
        assert_eq!(m.plugin_name, "\u{201c}Acme HTTP\u{201d}");
        assert_eq!(m.plugin_id, "acme.http");
        assert_eq!(m.source, "installed by you");
        assert!(!m.write_option);
        assert!(m.has_project);
        let v6 = model(&request(network("::1"), None), None);
        assert_eq!(v6.target, "https://[::1]:8443");
        assert!(!v6.has_project);
        assert_eq!(Choice::Project.label(false), "For this session");
    }

    #[test]
    fn a_plugin_name_never_becomes_the_question() {
        // A name written to look like a different, harmless request.
        let mut r = request(network("evil.example"), Some("p"));
        r.plugin_name = quote("x wants to read README.md\nAllow");
        let m = model(&r, Some(PluginSource::Dev));
        assert!(!m.plugin_name.contains('\n'), "control characters replaced");
        assert!(m.plugin_name.starts_with('\u{201c}'));
        assert_eq!(
            m.target, "https://evil.example:8443",
            "the target is host data"
        );
        assert!(!m.action.contains("README"));
        assert_eq!(m.source, "loaded unpacked (development)");
    }

    #[test]
    fn workspace_prompts_offer_writes_only_when_declared() {
        let read = ConsentKind::Workspace {
            write: false,
            network: true,
        };
        let m = model(&request(read, Some("shop")), Some(PluginSource::Bundled));
        assert!(!m.write_option);
        assert_eq!(m.target, "\u{201c}shop\u{201d}");
        assert_eq!(m.notes.len(), 2, "combined risk wording");
        let write = ConsentKind::Workspace {
            write: true,
            network: false,
        };
        assert!(model(&request(write, Some("shop")), None).write_option);
        let edit = model(&request(ConsentKind::EditorWrite, None), None);
        assert_eq!(edit.target, "(no project open)");
    }

    #[test]
    fn every_choice_maps_to_its_scope() {
        let scopes: Vec<_> = Choice::ALL.iter().map(|c| c.answer(true)).collect();
        assert_eq!(
            scopes,
            [
                ConsentAnswer::Allow {
                    scope: GrantScope::Once,
                    write: true
                },
                ConsentAnswer::Allow {
                    scope: GrantScope::Project,
                    write: true
                },
                ConsentAnswer::Allow {
                    scope: GrantScope::Global,
                    write: true
                },
                ConsentAnswer::Deny,
            ]
        );
        let labels: Vec<_> = Choice::ALL.iter().map(|c| c.label(true)).collect();
        assert_eq!(
            labels,
            ["Allow once", "Always for this project", "Always", "Deny"]
        );
        assert_eq!(describe(ConsentAnswer::Dismissed), "dismissed");
    }

    #[test]
    fn the_provider_blocks_until_the_ui_answers() {
        let (provider, asks) = AppConsent::new();
        let r = request(network("a.test"), None);
        let ui = std::thread::spawn(move || {
            let ask = asks.recv_blocking().unwrap();
            assert_eq!(ask.request.plugin.as_str(), "acme.http");
            ask.reply.send(ConsentAnswer::Deny);
            // A cancelled call: the provider stops waiting.
            let ask = asks.recv_blocking().unwrap();
            ask.cancel.cancel();
            std::thread::sleep(Duration::from_millis(50));
            drop(ask);
            asks
        });
        assert_eq!(provider.ask(&r, &CancelToken::new()), ConsentAnswer::Deny);
        assert_eq!(
            provider.ask(&r, &CancelToken::new()),
            ConsentAnswer::Dismissed
        );
        let asks = ui.join().unwrap();
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(provider.ask(&r, &cancelled), ConsentAnswer::Dismissed);
        assert!(asks.is_empty(), "nothing is shown for a cancelled call");
        drop(asks);
        assert_eq!(
            provider.ask(&r, &CancelToken::new()),
            ConsentAnswer::Dismissed,
            "no window: dismissed"
        );
    }
}
