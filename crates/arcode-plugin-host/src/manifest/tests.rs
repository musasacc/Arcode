//! Manifest tests. `testdata/*.toml` are the two reference manifests copied
//! verbatim from docs/PLUGINS.md §3.

use super::*;

const HTTP: &str = include_str!("testdata/http-client.toml");
const DB: &str = include_str!("testdata/db-viewer.toml");

fn bundled(text: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(text, PluginSource::Bundled)
}

const MINIMAL: &str = r#"
[plugin]
id = "acme.tool"
name = "Tool"
version = "1.0.0"
api = "0.1"
"#;

fn with(extra: &str) -> String {
    format!("{MINIMAL}\n{extra}")
}

fn invalid_message(text: &str) -> String {
    match Manifest::parse(text, PluginSource::Installed) {
        Err(ManifestError::Invalid(m) | ManifestError::Syntax(m)) => m,
        other => panic!("expected an invalid manifest, got {other:?}"),
    }
}

#[test]
fn parses_the_http_client_reference_manifest() {
    let m = bundled(HTTP).unwrap();
    assert_eq!(m.id().as_str(), "arcode.http-client");
    assert_eq!(m.plugin.entry, "plugin.wasm");
    assert_eq!(m.capabilities.workspace, WorkspaceAccess::Read);
    assert_eq!(m.capabilities.editor, EditorAccess::Context);
    assert!(!m.capabilities.editor_write);
    let net = m.capabilities.network.as_ref().unwrap();
    assert_eq!(
        (net.hosts.as_slice(), net.background),
        (&["*".to_owned()][..], false)
    );
    assert_eq!(
        m.limits,
        Limits {
            memory_mb: 64,
            task_seconds: 60
        }
    );
    let c = &m.contributes;
    assert_eq!(c.languages[0].extensions, ["http", "rest"]);
    assert_eq!(c.languages[0].line_comment.as_deref(), Some("#"));
    assert_eq!(c.commands.len(), 2);
    assert_eq!(c.commands[0].when, Some(When::Language("http".into())));
    assert_eq!(c.commands[0].key.as_deref(), Some("secondary-enter"));
    assert_eq!(c.panels[0].location, PanelLocation::EditorBeside);
    assert_eq!(c.settings[0].kind, SettingType::Integer);
    let defaults = m.default_settings();
    assert_eq!(defaults["timeout-ms"], 30000);
    assert_eq!(defaults["follow-redirects"], true);
}

#[test]
fn parses_the_db_viewer_reference_manifest() {
    let m = bundled(DB).unwrap();
    assert_eq!(m.id().as_str(), "arcode.db-viewer");
    assert_eq!(m.capabilities.editor, EditorAccess::None);
    assert!(m.capabilities.network.is_none());
    assert_eq!(
        m.limits,
        Limits {
            memory_mb: 128,
            task_seconds: 300
        }
    );
    assert_eq!(
        m.contributes.openers[0].patterns,
        ["*.sqlite", "*.sqlite3", "*.db"]
    );
    assert_eq!(m.contributes.panels[0].location, PanelLocation::Sidebar);
    assert_eq!(m.contributes.panels[0].icon.as_deref(), Some("database"));
    assert_eq!(m.default_settings()["page-size"], 200);
}

#[test]
fn minimal_manifest_defaults_to_no_capabilities() {
    let m = Manifest::parse(MINIMAL, PluginSource::Installed).unwrap();
    assert_eq!(m.capabilities, Capabilities::default());
    assert_eq!(m.limits, Limits::default());
    assert!(
        m.activation_events().is_empty(),
        "no contributions, no activation"
    );
}

#[test]
fn unknown_keys_are_errors_at_every_level() {
    for extra in [
        "startup = true",
        "[capabilities]\nprocess = true",
        "[capabilities]\nnetwork = { hosts = [\"*\"], sockets = true }",
        "[limits]\ncpu = 1",
        "[contributes]\nthemes = []",
        "[[contributes.commands]]\nid = \"a\"\ntitle = \"A\"\nicon = \"x\"",
        "[[contributes.panels]]\nid = \"a\"\ntitle = \"A\"\nlocation = \"bottom\"\nwidth = 3",
        "[[contributes.settings]]\nkey = \"a\"\ntype = \"boolean\"\ndefault = true\nscope = \"x\"",
    ] {
        let text = if extra.starts_with('[') {
            with(extra)
        } else {
            MINIMAL.replace("api = \"0.1\"", &format!("api = \"0.1\"\n{extra}"))
        };
        let message = invalid_message(&text);
        assert!(message.contains("unknown field"), "{extra}: {message}");
    }
}

#[test]
fn id_regex_is_enforced() {
    for bad in ["ab", "Acme.tool", "acme_tool", "-acme", "acme tool"] {
        let message = invalid_message(&MINIMAL.replace("acme.tool", bad));
        assert!(message.contains("invalid plugin id"), "{bad}: {message}");
    }
}

#[test]
fn reserved_prefix_only_for_bundled() {
    let text = MINIMAL.replace("acme.tool", "arcode.fake");
    assert!(matches!(
        Manifest::parse(&text, PluginSource::Installed),
        Err(ManifestError::ReservedId(_))
    ));
    assert!(matches!(
        Manifest::parse(&text, PluginSource::Dev),
        Err(ManifestError::ReservedId(_))
    ));
    assert!(Manifest::parse(&text, PluginSource::Bundled).is_ok());
}

#[test]
fn unsupported_api_is_refused_with_a_clear_message() {
    let err = Manifest::parse(
        &MINIMAL.replace("\"0.1\"", "\"0.2\""),
        PluginSource::Installed,
    )
    .unwrap_err();
    assert_eq!(
        err,
        ManifestError::UnsupportedApi {
            requested: "0.2".into()
        }
    );
    assert_eq!(
        err.to_string(),
        "plugin requires plugin API 0.2; this Arcode supports 0.1"
    );
}

#[test]
fn semantic_rules() {
    let cases = [
        (
            "[[contributes.commands]]\nid = \"a\"\ntitle = \"A\"\n[[contributes.commands]]\nid = \"a\"\ntitle = \"B\"",
            "duplicate command",
        ),
        (
            "[[contributes.commands]]\nid = \"Send\"\ntitle = \"A\"",
            "invalid command id",
        ),
        (
            "[[contributes.commands]]\nid = \"a\"\ntitle = \"\"",
            "must not be empty",
        ),
        (
            "[[contributes.commands]]\nid = \"a\"\ntitle = \"A\"\nwhen = \"editorFocus\"",
            "unsupported `when`",
        ),
        (
            "[[contributes.panels]]\nid = \"p\"\ntitle = \"P\"\nlocation = \"bottom\"\nicon = \"../x.svg\"",
            "invalid icon",
        ),
        (
            "[[contributes.panels]]\nid = \"p\"\ntitle = \"P\"\nlocation = \"floating\"",
            "unknown variant",
        ),
        (
            "[[contributes.openers]]\nid = \"o\"\ntitle = \"O\"\npatterns = [\"[\"]",
            "invalid opener pattern",
        ),
        (
            "[[contributes.openers]]\nid = \"o\"\ntitle = \"O\"\npatterns = []",
            "no patterns",
        ),
        (
            "[[contributes.languages]]\nid = \"x\"\nname = \"X\"\nextensions = [\".x\"]",
            "invalid extension",
        ),
        (
            "[[contributes.code-lenses]]\nlanguages = []",
            "must not be empty",
        ),
        (
            "[[contributes.settings]]\nkey = \"a\"\ntype = \"integer\"\ndefault = \"x\"",
            "does not match",
        ),
        (
            "[[contributes.settings]]\nkey = \"a\"\ntype = \"enum\"\ndefault = \"x\"",
            "needs non-empty `options`",
        ),
        (
            "[[contributes.settings]]\nkey = \"a\"\ntype = \"enum\"\ndefault = \"x\"\noptions = [\"y\"]",
            "not one of its options",
        ),
        (
            "[[contributes.settings]]\nkey = \"a\"\ntype = \"string\"\ndefault = \"x\"\noptions = [\"x\"]",
            "only enum",
        ),
        (
            "[capabilities]\nnetwork = { hosts = [] }",
            "at least one host",
        ),
        (
            "[capabilities]\nnetwork = { hosts = [\"http://x\"] }",
            "invalid network host",
        ),
        (
            "[capabilities]\nworkspace = \"everything\"",
            "unknown variant",
        ),
    ];
    for (extra, expected) in cases {
        let message = invalid_message(&with(extra));
        assert!(message.contains(expected), "{extra}\n=> {message}");
    }
    let message = invalid_message(&MINIMAL.replace("name = \"Tool\"", "name = \"Tool\\u0007\""));
    assert!(message.contains("control characters"), "{message}");
    let message = invalid_message(
        &MINIMAL.replace("api = \"0.1\"", "api = \"0.1\"\nentry = \"../evil.wasm\""),
    );
    assert!(message.contains("plugin.entry"), "{message}");
}

#[test]
fn enum_settings_with_valid_default_parse() {
    let m = Manifest::parse(
        &with("[[contributes.settings]]\nkey = \"mode\"\ntype = \"enum\"\ndefault = \"fast\"\noptions = [\"fast\", \"safe\"]"),
        PluginSource::Installed,
    )
    .unwrap();
    assert_eq!(m.default_settings()["mode"], "fast");
}

#[test]
fn contributions_are_namespaced() {
    let m = bundled(HTTP).unwrap();
    let send = m.id().qualify(&m.contributes.commands[0].id);
    assert_eq!(send.as_str(), "arcode.http-client/send");
    assert_eq!(send.local(), "send");
}

#[test]
fn activation_events_are_derived_from_contributions() {
    let http = bundled(HTTP).unwrap();
    let q = |s: &str| QualifiedId::parse(s).unwrap();
    assert_eq!(
        http.activation_events(),
        vec![
            ActivationEvent::Command(q("arcode.http-client/select-environment")),
            ActivationEvent::Command(q("arcode.http-client/send")),
            ActivationEvent::LensLanguage("http".into()),
            ActivationEvent::Panel(q("arcode.http-client/response")),
        ]
    );
    let db = bundled(DB).unwrap();
    assert_eq!(
        db.activation_events(),
        vec![
            ActivationEvent::Command(q("arcode.db-viewer/new-query")),
            ActivationEvent::Command(q("arcode.db-viewer/refresh")),
            ActivationEvent::Opener(q("arcode.db-viewer/sqlite")),
            ActivationEvent::Panel(q("arcode.db-viewer/databases")),
        ]
    );
}

#[test]
fn limits_from_manifests_are_clamped() {
    let db = bundled(DB).unwrap();
    let eff = db.limits.clamp(&UserLimits {
        max_memory_mb: 100,
        max_task_seconds: 120,
    });
    assert_eq!(
        eff,
        EffectiveLimits {
            memory_bytes: 100 * 1024 * 1024,
            task_seconds: 120
        }
    );
}
