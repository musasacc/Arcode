//! The status bar pickers: language (and file association), line ending
//! and indentation of the active file.

use std::path::Path;

use arcode_buffer::LineEnding;
use arcode_editor::{Editor, IndentOverride};
use arcode_syntax::Language;
use arcode_ui_kit::{IconName, settings};
use gpui::{AppContext, Context, Entity, Focusable, Window};

use super::Workspace;
use super::languages::{apply_detected_indent, configured_indent, sorted_languages};
use crate::languages::indent;
use crate::languages::resolve::{association_pattern, language_for, language_named};
use crate::languages::status_items::{indent_label, line_ending_label};
use crate::languages::{IndentChoice, LanguageChoice, PickPurpose};
use crate::list_picker::{ListPicker, ListPickerEvent, PickItem};

impl Workspace {
    fn show_list_picker(
        &mut self,
        placeholder: &'static str,
        icon: IconName,
        items: Vec<PickItem>,
        purpose: PickPurpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.new(|cx| ListPicker::new(placeholder, icon, items, cx));
        let sub = cx.subscribe_in(&picker, window, |ws, _, event, window, cx| {
            ws.list_picked(event.clone(), window, cx)
        });
        self.subs.set("list_picker", sub);
        window.focus(&picker.focus_handle(cx));
        self.langs.picker = Some((picker, purpose));
        cx.notify();
    }

    /// Closes the status bar picker (if open) and focuses the editor.
    pub(super) fn close_list_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.langs.picker.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn require_editor(&mut self, cx: &mut Context<Self>) -> Option<Entity<Editor>> {
        let editor = self.active_editor(cx);
        if editor.is_none() {
            self.show_toast("Open a file first", cx);
        }
        editor
    }

    /// Opens the language picker for the active file.
    pub fn open_language_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.require_editor(cx) else {
            return;
        };
        let e = editor.read(cx);
        let current = e.language();
        let path = e.path().map(Path::to_path_buf);
        let auto = path
            .as_deref()
            .map(|p| language_for(p, settings(cx)))
            .unwrap_or_else(arcode_syntax::plain_text);
        let mut choices = vec![LanguageChoice::Auto];
        let mut items = vec![PickItem::new(
            "Auto Detect",
            format!("{} (from the file name)", auto.name),
        )];
        if let Some(pattern) = path.as_deref().and_then(association_pattern) {
            choices.push(LanguageChoice::Associate);
            items.push(PickItem::new(
                format!("Configure File Association for '{pattern}'…"),
                "always open these files as one language",
            ));
        }
        for language in sorted_languages() {
            choices.push(LanguageChoice::Language(language));
            items.push(
                PickItem::new(language.name, language.id).current(std::ptr::eq(language, current)),
            );
        }
        let purpose = PickPurpose::Language {
            editor: editor.downgrade(),
            choices,
            associate: None,
        };
        self.show_list_picker(
            "Select Language Mode",
            IconName::FileText,
            items,
            purpose,
            window,
            cx,
        );
    }

    fn open_association_picker(
        &mut self,
        editor: &Entity<Editor>,
        pattern: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = settings(cx)
            .files
            .associations
            .get(&pattern)
            .and_then(|v| language_named(v));
        let languages = sorted_languages();
        let items = languages
            .iter()
            .map(|l| {
                PickItem::new(l.name, l.id).current(current.is_some_and(|c| std::ptr::eq(c, *l)))
            })
            .collect();
        let purpose = PickPurpose::Language {
            editor: editor.downgrade(),
            choices: languages
                .into_iter()
                .map(LanguageChoice::Language)
                .collect(),
            associate: Some(pattern),
        };
        self.show_list_picker(
            "Language to use for these files",
            IconName::FileText,
            items,
            purpose,
            window,
            cx,
        );
    }

    /// Opens the line ending picker for the active file.
    pub fn open_line_ending_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.require_editor(cx) else {
            return;
        };
        let current = editor.read(cx).line_ending();
        let choices = vec![LineEnding::Lf, LineEnding::CrLf];
        let items = choices
            .iter()
            .map(|e| {
                let detail = match e {
                    LineEnding::Lf => "Linux, macOS (\\n)",
                    LineEnding::CrLf => "Windows (\\r\\n)",
                };
                PickItem::new(line_ending_label(*e), detail).current(*e == current)
            })
            .collect();
        let purpose = PickPurpose::LineEnding {
            editor: editor.downgrade(),
            choices,
        };
        self.show_list_picker(
            "Select End of Line Sequence",
            IconName::Type,
            items,
            purpose,
            window,
            cx,
        );
    }

    /// Opens the indentation picker for the active file.
    pub fn open_indent_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.require_editor(cx) else {
            return;
        };
        let e = editor.read(cx);
        let current = e.indentation(cx);
        let configured = configured_indent(e, cx);
        let mut choices: Vec<IndentChoice> = [2, 4, 8]
            .map(|n| IndentChoice::Set(IndentOverride::spaces(n)))
            .to_vec();
        choices.extend([2, 4, 8].map(|n| IndentChoice::Set(IndentOverride::tabs(n))));
        choices.extend([
            IndentChoice::Detect,
            IndentChoice::Reset,
            IndentChoice::ToSpaces,
            IndentChoice::ToTabs,
        ]);
        let items = choices
            .iter()
            .map(|choice| match choice {
                IndentChoice::Set(o) if o.uses_spaces() => {
                    PickItem::new(format!("Indent Using Spaces: {}", o.size()), "")
                        .current(*o == current)
                }
                IndentChoice::Set(o) => {
                    PickItem::new(format!("Indent Using Tabs: Tab Size {}", o.tab_width), "")
                        .current(*o == current)
                }
                IndentChoice::Detect => PickItem::new("Detect Indentation from Content", ""),
                IndentChoice::Reset => {
                    PickItem::new("Use Language Settings", indent_label(configured))
                }
                IndentChoice::ToSpaces => {
                    PickItem::new("Convert Indentation to Spaces", "rewrites leading tabs")
                }
                IndentChoice::ToTabs => {
                    PickItem::new("Convert Indentation to Tabs", "rewrites leading spaces")
                }
            })
            .collect();
        let purpose = PickPurpose::Indentation {
            editor: editor.downgrade(),
            choices,
        };
        self.show_list_picker(
            "Select Indentation",
            IconName::Rows,
            items,
            purpose,
            window,
            cx,
        );
    }

    fn list_picked(&mut self, event: ListPickerEvent, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(event, ListPickerEvent::Highlighted(_)) {
            return;
        }
        let Some((_, purpose)) = self.langs.picker.take() else {
            return;
        };
        self.active_pane().read(cx).focus(window, cx);
        cx.notify();
        let ListPickerEvent::Picked(ix) = event else {
            return;
        };
        match purpose {
            PickPurpose::Language {
                editor,
                choices,
                associate,
            } => {
                let (Some(editor), Some(choice)) = (editor.upgrade(), choices.get(ix).copied())
                else {
                    return;
                };
                self.language_picked(&editor, choice, associate, window, cx);
            }
            PickPurpose::LineEnding { editor, choices } => {
                if let (Some(editor), Some(ending)) = (editor.upgrade(), choices.get(ix)) {
                    editor.update(cx, |e, cx| e.set_line_ending(*ending, cx));
                    self.refresh_status(cx);
                }
            }
            PickPurpose::Indentation { editor, choices } => {
                if let (Some(editor), Some(choice)) = (editor.upgrade(), choices.get(ix)) {
                    self.apply_indent_choice(&editor, *choice, cx);
                }
            }
        }
    }

    fn language_picked(
        &mut self,
        editor: &Entity<Editor>,
        choice: LanguageChoice,
        associate: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        match (choice, associate) {
            (LanguageChoice::Language(language), Some(pattern)) => {
                self.langs.manual.remove(&id);
                self.write_association(pattern, language, cx);
            }
            (LanguageChoice::Associate, _) => {
                let pattern = editor.read(cx).path().and_then(association_pattern);
                if let Some(pattern) = pattern {
                    self.open_association_picker(editor, pattern, window, cx);
                }
            }
            (LanguageChoice::Auto, _) => {
                self.langs.manual.remove(&id);
                self.reapply_association(editor, cx);
            }
            (LanguageChoice::Language(language), None) => {
                self.langs.manual.insert(id, language);
                self.set_editor_language(editor, language, cx);
            }
        }
    }

    /// Writes `pattern = language` to `[files] associations`; the reload
    /// that follows applies it to every open file it matches.
    fn write_association(
        &mut self,
        pattern: String,
        language: &'static Language,
        cx: &mut Context<Self>,
    ) {
        let id = language.id;
        let shown = pattern.clone();
        // Queued with the Settings page's writes; the result is applied.
        let write = crate::prefs::store::edit_config(
            move |source| {
                arcode_config::config::edit::set_file_association(source, &pattern, Some(id))
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |ws, cx| match result {
                Ok(()) => {
                    ws.show_toast(format!("{shown} files now open as {}", language.name), cx);
                }
                Err(err) => ws.show_toast_as(err, super::ToastLevel::Error, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn apply_indent_choice(
        &mut self,
        editor: &Entity<Editor>,
        choice: IndentChoice,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |e, cx| match choice {
            IndentChoice::Set(indent) => e.set_indent_override(Some(indent), cx),
            IndentChoice::Detect => apply_detected_indent(e, cx),
            IndentChoice::Reset => e.set_indent_override(None, cx),
            IndentChoice::ToSpaces | IndentChoice::ToTabs => {
                let to_spaces = choice == IndentChoice::ToSpaces;
                let current = e.indentation(cx);
                let edits = indent::convert_edits(e.buffer().text(), to_spaces, current.tab_width);
                e.apply_edits(edits, cx);
                let indent = if to_spaces {
                    IndentOverride::spaces(current.size())
                } else {
                    IndentOverride::tabs(current.tab_width)
                };
                e.set_indent_override(Some(indent), cx);
            }
        });
        self.refresh_status(cx);
    }
}
