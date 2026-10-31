//! The language, line ending and indentation of open files: file
//! associations and indentation detection on open, the status bar
//! pickers, and the Languages page.

use arcode_editor::{Editor, IndentOverride};
use arcode_syntax::Language;
use arcode_ui_kit::settings;
use gpui::{AppContext, Context, Entity, Subscription};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::languages::actions::*;
use crate::languages::indent::{self, IndentGuess};
use crate::languages::resolve::language_for;
use crate::languages::view::{LanguagesEvent, LanguagesView};
use crate::languages::{IndentChoice, LangParts, PackageBanner};
use crate::list_picker::ListPicker;

/// The indentation the language settings give `editor` (no override).
pub(super) fn configured_indent(editor: &Editor, cx: &gpui::App) -> IndentOverride {
    let config = settings(cx);
    let id = editor.language().id;
    let size = config.effective_tab_size(id).clamp(1, 16);
    if config.effective_insert_spaces(id) {
        IndentOverride::spaces(size)
    } else {
        IndentOverride::tabs(size)
    }
}

/// Sets `editor`'s indentation from its content (or clears the override
/// when the content agrees with the settings or says nothing).
pub(super) fn apply_detected_indent(editor: &mut Editor, cx: &mut Context<Editor>) {
    let configured = configured_indent(editor, cx);
    let detected = match indent::detect_rope(editor.buffer().text()) {
        Some(IndentGuess::Tabs) => IndentOverride::tabs(configured.tab_width),
        Some(IndentGuess::Spaces(n)) => IndentOverride::spaces(n),
        None => configured,
    };
    let value = (detected != configured).then_some(detected);
    editor.set_indent_override(value, cx);
}

impl Workspace {
    /// The Languages page and the language state of a new workspace.
    pub(super) fn new_lang_parts(cx: &mut Context<Self>) -> (LangParts, Vec<Subscription>) {
        let view = cx.new(LanguagesView::new);
        let subs = vec![
            cx.subscribe(&view, |ws, _, event: &LanguagesEvent, cx| {
                let LanguagesEvent::Message(text) = event;
                ws.show_toast(text.clone(), cx);
            }),
            cx.observe_global::<arcode_ui_kit::Settings>(|ws, cx| {
                ws.languages_settings_changed(cx)
            }),
        ];
        let parts = LangParts {
            view,
            picker: None,
            manual: Default::default(),
            banner: PackageBanner::default(),
        };
        (parts, subs)
    }

    /// The Languages page.
    pub fn languages_view(&self) -> &Entity<LanguagesView> {
        &self.langs.view
    }

    /// The open status bar picker (language, line ending, indentation).
    pub fn list_picker(&self) -> Option<&Entity<ListPicker>> {
        self.langs.picker.as_ref().map(|(picker, _)| picker)
    }

    /// The package banner state.
    pub fn package_banner(&self) -> &PackageBanner {
        &self.langs.banner
    }

    /// Gives a newly opened editor the language its file association
    /// names and, with `editor.detect_indentation`, the indentation its
    /// content uses. Runs before the language servers see the file.
    pub(super) fn prepare_editor_language(
        &mut self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) {
        let config = settings(cx);
        let detect = config.editor.detect_indentation;
        let language = editor.read(cx).path().map(|p| language_for(p, config));
        editor.update(cx, |e, cx| {
            if let Some(language) = language {
                e.set_language(language, cx);
            }
            if detect {
                apply_detected_indent(e, cx);
            }
        });
    }

    /// Switches `editor` to `language` and opens it again with the
    /// language servers of the new language.
    pub(super) fn set_editor_language(
        &mut self,
        editor: &Entity<Editor>,
        language: &'static Language,
        cx: &mut Context<Self>,
    ) {
        if std::ptr::eq(editor.read(cx).language(), language) {
            return;
        }
        editor.update(cx, |e, cx| {
            e.set_language(language, cx);
            // The bridge installs these again if the new language has
            // servers.
            e.set_completion_provider(None);
            e.set_hover_provider(None);
            e.set_language_services(None, cx);
        });
        // The old language's servers are closed and their diagnostics
        // dropped; the new language's servers open the file again.
        self.lsp
            .update(cx, |lsp, cx| lsp.editor_language_changed(editor, cx));
        self.debug
            .debugger
            .update(cx, |d, cx| d.language_changed(editor, cx));
        self.refresh_status(cx);
    }

    /// The language `editor` should have again (after a rename or a
    /// settings change): the one picked by hand if any, else the one its
    /// file association or name gives.
    pub(super) fn reapply_association(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let language = match self.langs.manual.get(&editor.entity_id()) {
            Some(language) => Some(*language),
            None => editor
                .read(cx)
                .path()
                .map(|p| language_for(p, settings(cx))),
        };
        if let Some(language) = language {
            self.set_editor_language(editor, language, cx);
        }
    }

    fn languages_settings_changed(&mut self, cx: &mut Context<Self>) {
        for editor in self.editors(cx) {
            self.reapply_association(&editor, cx);
        }
        self.schedule_package_check(cx);
        self.refresh_status(cx);
    }

    /// Shows the Languages page.
    pub fn show_languages(&mut self, cx: &mut Context<Self>) {
        self.activity = Activity::Plugins;
        self.sidebar_visible = true;
        self.langs.view.update(cx, |v, cx| v.shown(cx));
        cx.notify();
    }

    pub(super) fn register_language_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|ws, _: &SelectLanguageMode, window, cx| {
                ws.open_language_picker(window, cx)
            }),
        )
        .on_action(cx.listener(|ws, _: &ChangeLineEnding, window, cx| {
            ws.open_line_ending_picker(window, cx)
        }))
        .on_action(
            cx.listener(|ws, _: &ChangeIndentation, window, cx| ws.open_indent_picker(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &ConvertIndentationToSpaces, _, cx| {
            ws.indent_command(IndentChoice::ToSpaces, cx)
        }))
        .on_action(cx.listener(|ws, _: &ConvertIndentationToTabs, _, cx| {
            ws.indent_command(IndentChoice::ToTabs, cx)
        }))
        .on_action(cx.listener(|ws, _: &DetectIndentation, _, cx| {
            ws.indent_command(IndentChoice::Detect, cx)
        }))
        .on_action(cx.listener(|ws, _: &ShowLanguages, _, cx| ws.show_languages(cx)))
        .on_action(
            cx.listener(|ws, _: &InstallPackages, window, cx| ws.install_packages(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &CheckPackages, _, cx| ws.check_packages_now(cx)))
    }

    fn indent_command(&mut self, choice: IndentChoice, cx: &mut Context<Self>) {
        if let Some(editor) = self.require_editor(cx) {
            self.apply_indent_choice(&editor, choice, cx);
        }
    }
}

/// Every language, by name (plain text included).
pub(super) fn sorted_languages() -> Vec<&'static Language> {
    let mut languages = arcode_syntax::all_languages().to_vec();
    languages.sort_by_key(|l| l.name.to_lowercase());
    languages
}
