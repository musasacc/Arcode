//! The page's title row with its "Filter settings" field: while the field
//! holds text, the page shows the matching rows of every section instead
//! of one section.
//!
//! Rows are built by free functions ([`super::widgets::setting_row`]), so
//! the query is handed to them for the duration of one draw through a
//! thread-local scope rather than through every signature. Nothing is
//! stored between frames.

use std::cell::{Cell, RefCell};

use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, icon_size, type_scale};
use arcode_ui_kit::{IconName, icon, input_frame, theme};
use gpui::{Context, Focusable, Window, div, prelude::*, px};

use super::{Section, SettingsPage};

thread_local! {
    static QUERY: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    static HITS: Cell<usize> = const { Cell::new(0) };
}

/// The lower-case words of `query` (empty when it is blank).
pub(crate) fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether a row labelled `label` and described by `description` contains
/// every word.
pub(crate) fn row_matches(words: &[String], label: &str, description: &str) -> bool {
    let text = format!("{label} {description}").to_lowercase();
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
pub(crate) fn admit(label: &str, description: &str) -> bool {
    QUERY.with(|q| match &*q.borrow() {
        None => true,
        Some(words) => {
            let hit = row_matches(words, label, description);
            if hit {
                HITS.with(|h| h.set(h.get() + 1));
            }
            hit
        }
    })
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
        assert!(row_matches(&w, "Low-end mode", "Flat popups"));
        assert!(row_matches(
            &words("popups flat"),
            "Low-end mode",
            "Flat popups"
        ));
        assert!(!row_matches(
            &words("low tabs"),
            "Low-end mode",
            "Flat popups"
        ));
        assert!(row_matches(&words(""), "Anything", ""));
    }

    #[test]
    fn a_scope_counts_hits_and_ends_on_drop() {
        assert!(!active());
        assert!(admit("Tab size", ""));
        {
            let scope = Scope::new(words("tab"));
            assert!(active());
            assert!(admit("Tab size", ""));
            assert!(!admit("Font", "Family"));
            assert!(admit("Insert spaces", "instead of tabs"));
            assert_eq!(scope.take_hits(), 2);
            assert_eq!(scope.take_hits(), 0);
        }
        assert!(!active());
        assert!(admit("Font", "Family"));
        // A blank query filters nothing.
        let _scope = Scope::new(words("  "));
        assert!(!active());
    }
}
