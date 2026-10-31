//! Review tests (plugins runtime wiring): plugin manifests that take over
//! built-in commands and shortcuts. Each failing test reproduces a bug
//! found in review.

use arcode_plugin_host::QualifiedId;
use gpui::{KeyContext, Keystroke, TestAppContext};

use super::client::PaletteCommand;
use super::commands::{RunPluginCommand, register_commands};

fn command(title: &str, key: Option<&str>) -> PaletteCommand {
    PaletteCommand {
        id: QualifiedId::parse("evil.plugin/run").expect("valid id"),
        title: title.into(),
        when: None,
        key: key.map(str::to_owned),
    }
}

/// A third-party manifest with `title = "File: Save"` replaces the
/// built-in palette entry (the registry replaces equal labels): picking
/// "File: Save" in the palette then runs the plugin command, as a user
/// gesture, and the real Save is gone from the palette.
#[gpui::test]
fn review_plugin_title_replaces_builtin_palette_command(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::commands::init(cx);
        let builtin = |cx: &gpui::App| {
            crate::commands::commands(cx)
                .iter()
                .filter(|c| c.label() == "File: Save")
                .map(|c| c.action_ref().name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(builtin(cx), ["editor::Save"], "precondition");
        register_commands(&[command("File: Save", None)], cx);
        assert!(
            builtin(cx).iter().any(|name| name == "editor::Save"),
            "a plugin title must not replace the built-in command; palette now runs {:?}",
            builtin(cx)
        );
    });
}

/// A manifest `key = "secondary-s"` is bound in the `Editor` context after
/// the editor's own bindings, so it wins over Save: Ctrl+S in every editor
/// runs the plugin command (a gesture) instead of saving. Same for
/// `secondary-c` (Copy) and every other built-in editor shortcut.
#[gpui::test]
fn review_manifest_key_overrides_builtin_editor_shortcut(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_editor::init(cx);
        crate::commands::init(cx);
        register_commands(&[command("Evil: Run", Some("secondary-s"))], cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let input = [Keystroke::parse("secondary-s").expect("keystroke")];
        let stack = [
            KeyContext::parse("Workspace").expect("context"),
            KeyContext::parse(arcode_editor::KEY_CONTEXT).expect("context"),
        ];
        let (bindings, _) = keymap.bindings_for_input(&input, &stack);
        let first = bindings.first().expect("Ctrl+S is bound in editors");
        assert!(
            first
                .action()
                .as_any()
                .downcast_ref::<RunPluginCommand>()
                .is_none(),
            "a plugin key must not shadow a built-in editor shortcut; Ctrl+S runs {}",
            first.action().name()
        );
    });
}

/// The host only lets an `editor = "context"` plugin read documents of a
/// language it contributes when that language is not built in, so plain
/// text stays private. The app then turns *every* plain-text file whose
/// extension a plugin lists into that plugin's language: a manifest with
/// `extensions = ["key", "pem", "txt"]` reads every private key or note
/// the user opens (lens calls hand it the document) without any prompt.
#[test]
fn review_plugin_language_claims_plain_text_files() {
    use std::path::Path;

    use super::snapshot::{ContributedLanguage, language_for};

    let plain = arcode_syntax::plain_text().id;
    let claimed: Vec<ContributedLanguage> = ["key", "pem", "txt"]
        .iter()
        .map(|ext| ContributedLanguage {
            extension: (*ext).into(),
            id: "evil".into(),
            name: "Evil".into(),
        })
        .collect();
    for file in ["/p/server.key", "/p/cert.pem", "/p/notes.txt"] {
        assert_eq!(
            arcode_syntax::language_for_path(Path::new(file)).id,
            plain,
            "precondition: Arcode opens {file} as plain text"
        );
        assert_eq!(
            language_for(Some(Path::new(file)), plain, &claimed),
            plain,
            "{file} becomes readable by the plugin without consent"
        );
    }
}
