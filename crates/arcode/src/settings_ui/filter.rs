//! The page's title row with its "Filter settings" field and an "Open
//! config.toml" button: while the field holds text, the page shows the matching rows of every section instead
//! of one section.
//!
//! Rows are built by free functions ([`super::widgets::setting_row`]), so
//! the query is handed to them for the duration of one draw through a
//! thread-local scope rather than through every signature. Nothing is
//! stored between frames.

use std::cell::{Cell, RefCell};

use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, icon_size, type_scale};
use arcode_ui_kit::{IconName, button, icon, icon_button, input_frame, theme};
use gpui::{Context, Focusable, SharedString, Window, div, prelude::*, px};

use super::widgets::{RowKey, keyed_setting_row};
use super::{Section, SettingsEvent, SettingsPage};

thread_local! {
    static QUERY: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    static HITS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Keys of the rows drawn while a test collects them.
    static DRAWN_KEYS: RefCell<Option<std::collections::BTreeSet<String>>> =
        const { RefCell::new(None) };
}

/// Other words people use for a setting, matched like its label.
const SYNONYMS: &[(&str, &str)] = &[
    ("ui.theme", "colour color scheme dark light"),
    ("ui.font_size", "zoom scale interface"),
    ("ui.low_end_mode", "performance slow battery power saving"),
    ("editor.font_size", "zoom text"),
    ("editor.word_wrap", "soft wrap line wrap"),
    ("editor.minimap", "overview"),
    ("editor.cursor_blink", "caret"),
    ("editor.show_line_numbers", "gutter"),
    ("editor.insert_spaces", "indent indentation soft tabs"),
    (
        "editor.keymap",
        "vim bindings shortcuts modal editor.vim_mode",
    ),
    ("editor.format_on_save", "formatter prettier rustfmt black"),
    ("editor.inlay_hints", "types parameter names"),
    ("editor.inline_diagnostics", "errors warnings problems lens"),
    (
        "editor.fade_unused",
        "dim grey gray opacity strikethrough strike deprecated",
    ),
    (
        "editor.completion.auto_trigger",
        "autocomplete intellisense suggestions",
    ),
    ("git.inline_blame", "annotate author"),
    ("terminal.shell", "bash zsh fish powershell cmd"),
    ("search.exclude", "ignore skip globs"),
    ("todo.tags", "fixme hack note"),
    ("workspace.confirm_delete", "trash remove"),
    ("plugins.network", "internet http offline"),
];

/// Settings without a row of their own (tables of patterns or adapters):
/// the filter finds them by key, label or note and points to
/// `config.toml`. Key, label, note.
pub(crate) const ELSEWHERE: &[(&str, &str, &str)] = &[
    (
        "files.associations",
        "File associations",
        "File patterns and their language, set in config.toml. Choosing a file's language in the status bar adds one.",
    ),
    (
        "debug.adapters",
        "Debug adapters",
        "The debug adapter of each language, set in config.toml.",
    ),
    (
        "debug.adapter_paths",
        "Debug adapter paths",
        "The program of each debug adapter, set in config.toml.",
    ),
];

/// The synonyms of the setting at `key` (empty when it has none).
pub(crate) fn synonyms(key: &str) -> &'static str {
    SYNONYMS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("", |(_, words)| words)
}

/// The lower-case words of `query` (empty when it is blank).
pub(crate) fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether a row labelled `label`, described by `description` and bound
/// to the `config.toml` key `key` contains every word, in its text, its
/// key (`tab_size`, `editor.minimap`) or the key's synonyms.
pub(crate) fn row_matches(
    words: &[String],
    label: &str,
    description: &str,
    key: Option<&str>,
) -> bool {
    let key = key.unwrap_or("");
    let text = format!("{label} {description} {key} {}", synonyms(key)).to_lowercase();
    words.iter().all(|w| text.contains(w.as_str()))
}

/// Filters the rows drawn while it lives; dropping it ends the filter.
pub(crate) struct Scope(());

impl Scope {
    /// Starts filtering by `words` (nothing is filtered when empty).
    pub(crate) fn new(words: Vec<String>) -> Self {
        QUERY.with(|q| *q.borrow_mut() = (!words.is_empty()).then_some(words));
        HITS.with(|h| h.set(0));
        Scope(())
    }

    /// Rows admitted since the last call, and starts counting again.
    pub(crate) fn take_hits(&self) -> usize {
        HITS.with(|h| h.replace(0))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        QUERY.with(|q| q.borrow_mut().take());
    }
}

/// Whether a filter is active (group headings and decorations hide then).
pub(crate) fn active() -> bool {
    QUERY.with(|q| q.borrow().is_some())
}

/// Whether the row is shown: always without a filter, else when it
/// matches (and then it is counted).
pub(crate) fn admit(label: &str, description: &str, key: Option<&str>) -> bool {
    QUERY.with(|q| match &*q.borrow() {
        None => true,
        Some(words) => {
            let hit = row_matches(words, label, description, key);
            if hit {
                HITS.with(|h| h.set(h.get() + 1));
            }
            hit
        }
    })
}

/// Collects the keys of the rows drawn until [`take_drawn_keys`].
#[cfg(test)]
pub(crate) fn collect_drawn_keys() {
    DRAWN_KEYS.with(|k| *k.borrow_mut() = Some(Default::default()));
}

/// The keys collected since [`collect_drawn_keys`]; stops collecting.
#[cfg(test)]
pub(crate) fn take_drawn_keys() -> std::collections::BTreeSet<String> {
    DRAWN_KEYS.with(|k| k.borrow_mut().take().unwrap_or_default())
}

/// Notes that a row for `key` was built (while a test collects them).
#[cfg(test)]
pub(crate) fn record_key(key: &str) {
    DRAWN_KEYS.with(|k| {
        if let Some(keys) = k.borrow_mut().as_mut() {
            keys.insert(key.to_owned());
        }
    });
}

impl SettingsPage {
    /// The title row: the section name (or "Settings" while filtering) and
    /// the filter field at the right.
    pub(super) fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let filtering = self.filtering(cx);
        let focused = self.filter.focus_handle(cx).is_focused(window);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .pb(px(4.))
            .child(
                div()
                    .flex_1()
                    .text_size(type_scale(cx).heading)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .child(if filtering {
                        "Settings"
                    } else {
                        self.section.name()
                    }),
            )
            .child(
                input_frame(focused, false, cx)
                    .w(px(220.))
                    .child(
                        icon(IconName::Search)
                            .size(icon_size::TOOLBAR)
                            .color(c.text_muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .overflow_hidden()
                            .child(self.filter.clone()),
                    ),
            )
            .child(
                icon_button("settings-open-config", IconName::ExternalLink)
                    .tooltip("Open config.toml")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
            )
    }

    /// Every section's rows that match the filter, under section titles;
    /// sections without a match are left out. The Keymap has its own
    /// filter and is not searched here.
    pub(super) fn render_filtered(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let scope = Scope::new(words(self.filter.read(cx).text()));
        let mut out = div();
        let mut any = false;
        for section in Section::ALL {
            if section == Section::Keymap {
                continue;
            }
            let rows = self.render_section_of(section, cx);
            if scope.take_hits() > 0 {
                any = true;
                out = out
                    .child(
                        div()
                            .pt(px(22.))
                            .pb(px(2.))
                            .text_size(type_scale(cx).title)
                            .font_weight(WEIGHT_SEMIBOLD)
                            .child(section.name()),
                    )
                    .child(rows);
            }
        }
        drop(scope);
        let words = words(self.filter.read(cx).text());
        let elsewhere: Vec<_> = ELSEWHERE
            .iter()
            .filter(|(key, label, note)| row_matches(&words, label, note, Some(key)))
            .collect();
        if !elsewhere.is_empty() {
            any = true;
            out = out.child(
                div()
                    .pt(px(22.))
                    .pb(px(2.))
                    .text_size(type_scale(cx).title)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .child("In config.toml"),
            );
            for (key, label, note) in elsewhere {
                let open = button(
                    SharedString::from(format!("open-config-{key}")),
                    "Open config.toml",
                )
                .icon(IconName::FileText)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile)));
                let key = RowKey {
                    path: SharedString::from(*key),
                    reset: None,
                };
                out = out.child(keyed_setting_row(*label, *note, key, open, cx));
            }
        }
        if !any {
            out = out.child(
                div()
                    .pt(px(22.))
                    .text_size(type_scale(cx).body)
                    .text_color(c.text_muted)
                    .child("No settings match the filter."),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_match_every_word_in_label_or_description() {
        let w = words("  Low END ");
        assert_eq!(w, ["low", "end"]);
        assert!(row_matches(&w, "Low-end mode", "Flat popups", None));
        assert!(row_matches(
            &words("popups flat"),
            "Low-end mode",
            "Flat popups",
            None
        ));
        assert!(!row_matches(
            &words("low tabs"),
            "Low-end mode",
            "Flat popups",
            None
        ));
        assert!(row_matches(&words(""), "Anything", "", None));
    }

    #[test]
    fn rows_match_their_key_and_its_synonyms() {
        let tab = |query: &str| row_matches(&words(query), "Tab size", "", Some("editor.tab_size"));
        assert!(tab("tab_size"));
        assert!(tab("editor.tab_size"));
        assert!(tab("EDITOR.TAB"));
        assert!(!tab("editor.minimap"));
        let blink = |query: &str| {
            row_matches(
                &words(query),
                "Cursor blink",
                "",
                Some("editor.cursor_blink"),
            )
        };
        assert!(blink("caret"));
        assert!(blink("caret editor."));
        assert!(!blink("caret zoom"));
        let fade = |query: &str| {
            row_matches(
                &words(query),
                "Fade unused code",
                "Deprecated code is always struck through.",
                Some("editor.fade_unused"),
            )
        };
        for query in [
            "dim",
            "grey",
            "gray",
            "strikethrough",
            "unused",
            "deprecated",
        ] {
            assert!(fade(query), "{query}");
        }
        // Synonyms belong to a key: a row without one has none.
        assert!(!row_matches(&words("caret"), "Cursor blink", "", None));
        for (key, words) in SYNONYMS {
            assert!(key.contains('.') && !words.is_empty(), "{key}");
        }
    }

    #[test]
    fn a_scope_counts_hits_and_ends_on_drop() {
        assert!(!active());
        assert!(admit("Tab size", "", None));
        {
            let scope = Scope::new(words("tab"));
            assert!(active());
            assert!(admit("Tab size", "", None));
            assert!(!admit("Font", "Family", None));
            assert!(admit("Insert spaces", "instead of tabs", None));
            assert!(admit("Indent", "", Some("editor.tab_size")));
            assert_eq!(scope.take_hits(), 3);
            assert_eq!(scope.take_hits(), 0);
        }
        assert!(!active());
        assert!(admit("Font", "Family", None));
        // A blank query filters nothing.
        let _scope = Scope::new(words("  "));
        assert!(!active());
    }
}
