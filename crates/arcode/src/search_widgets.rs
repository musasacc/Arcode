//! Small pieces shared by the find bar and the Search view: option toggles
//! (match case, whole word, regex) and bordered input boxes.

use arcode_search::SearchQuery;
use arcode_ui_kit::{IconName, Tooltip, icon, theme};
use gpui::{
    AnyElement, AnyView, App, ClickEvent, ElementId, Entity, Focusable, SharedString, Window, div,
    prelude::*, px,
};

use crate::text_field::TextField;

/// The options shared by every search box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// Match case exactly.
    pub case_sensitive: bool,
    /// Only whole words.
    pub whole_word: bool,
    /// The pattern is a regular expression.
    pub regex: bool,
}

impl SearchOptions {
    /// A query for `pattern` with these options (no smart case: the toggle
    /// decides).
    pub fn query(self, pattern: &str) -> SearchQuery {
        SearchQuery {
            pattern: pattern.to_owned(),
            is_regex: self.regex,
            case_sensitive: self.case_sensitive,
            smart_case: false,
            whole_word: self.whole_word,
            ..SearchQuery::default()
        }
    }
}

/// Which option a toggle flips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchOption {
    /// [`SearchOptions::case_sensitive`].
    Case,
    /// [`SearchOptions::whole_word`].
    Word,
    /// [`SearchOptions::regex`].
    Regex,
}

impl SearchOption {
    /// All options in display order.
    pub const ALL: [SearchOption; 3] =
        [SearchOption::Case, SearchOption::Word, SearchOption::Regex];

    /// Flips this option in `opts`.
    pub fn toggle(self, opts: &mut SearchOptions) {
        match self {
            SearchOption::Case => opts.case_sensitive = !opts.case_sensitive,
            SearchOption::Word => opts.whole_word = !opts.whole_word,
            SearchOption::Regex => opts.regex = !opts.regex,
        }
    }

    /// Whether this option is on in `opts`.
    pub fn is_on(self, opts: &SearchOptions) -> bool {
        match self {
            SearchOption::Case => opts.case_sensitive,
            SearchOption::Word => opts.whole_word,
            SearchOption::Regex => opts.regex,
        }
    }

    fn icon(self) -> IconName {
        match self {
            SearchOption::Case => IconName::CaseSensitive,
            SearchOption::Word => IconName::WholeWord,
            SearchOption::Regex => IconName::Regex,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            SearchOption::Case => "Match Case (Alt+C)",
            SearchOption::Word => "Match Whole Word (Alt+W)",
            SearchOption::Regex => "Use Regular Expression (Alt+R)",
        }
    }
}

/// A small square toggle with an accent outline while on.
pub fn toggle_button(
    id: impl Into<ElementId>,
    icon_name: IconName,
    on: bool,
    tooltip: &'static str,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let c = theme(cx).colors.clone();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(20.))
        .rounded_sm()
        .border_1()
        .cursor_pointer()
        .when(on, |el| {
            el.border_color(c.accent).bg(c.accent.opacity(0.18))
        })
        .when(!on, |el| el.border_color(gpui::transparent_black()))
        .hover(|s| s.bg(c.hover))
        .child(
            icon(icon_name)
                .size(px(14.))
                .color(if on { c.text } else { c.text_muted }),
        )
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip, cx) })
        .on_click(on_click)
        .into_any_element()
}

/// The three option toggles; `on_toggle` receives the flipped option.
pub fn option_toggles(
    id_prefix: &'static str,
    opts: SearchOptions,
    cx: &App,
    on_toggle: impl Fn(SearchOption, &mut Window, &mut App) + Clone + 'static,
) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_0p5()
        .children(SearchOption::ALL.into_iter().map(|option| {
            let on_toggle = on_toggle.clone();
            toggle_button(
                SharedString::from(format!("{id_prefix}-{option:?}")),
                option.icon(),
                option.is_on(&opts),
                option.tooltip(),
                cx,
                move |_, window, cx| on_toggle(option, window, cx),
            )
        }))
}

/// A bordered single-line input box around `field`, with optional
/// trailing content (toggles). The border shows focus or an error.
pub fn field_box(
    field: &Entity<TextField>,
    error: bool,
    trailing: Option<AnyElement>,
    window: &Window,
    cx: &App,
) -> gpui::Div {
    let c = theme(cx).colors.clone();
    let focused = field.focus_handle(cx).is_focused(window);
    let border = if error {
        c.error
    } else if focused {
        c.border_focused
    } else {
        c.border
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .h(px(26.))
        .min_w_0()
        .pl_2()
        .pr_1()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(c.background)
        .child(div().flex_1().min_w_0().h_full().child(field.clone()))
        .children(trailing)
}

/// A search error on one line for a narrow view: the regex engine's
/// multi-line report (pattern, caret, message) becomes
/// `Invalid regular expression: unclosed group`.
pub fn short_error(message: &str) -> String {
    let lines: Vec<&str> = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let text = match (lines.first(), lines.last()) {
        (Some(first), Some(last)) if lines.len() > 1 => {
            let head = first.split(':').next().unwrap_or(first);
            let tail = last.strip_prefix("error:").unwrap_or(last).trim();
            format!("{head}: {tail}")
        }
        _ => message.trim().to_owned(),
    };
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_toggle_and_build_queries() {
        let mut opts = SearchOptions::default();
        SearchOption::Regex.toggle(&mut opts);
        SearchOption::Case.toggle(&mut opts);
        assert!(opts.regex && opts.case_sensitive && !opts.whole_word);
        let q = opts.query("a.b");
        assert!(q.is_regex && q.case_sensitive && !q.smart_case);
        SearchOption::Case.toggle(&mut opts);
        assert!(!SearchOption::Case.is_on(&opts));
    }

    #[test]
    fn regex_errors_fit_on_one_line() {
        let q = arcode_search::SearchQuery {
            is_regex: true,
            ..arcode_search::SearchQuery::new("(")
        };
        let err = q.validate().unwrap_err().to_string();
        assert!(err.contains('\n'));
        assert_eq!(
            short_error(&err),
            "Invalid regular expression: unclosed group"
        );
        assert_eq!(short_error("invalid glob `[`: x"), "Invalid glob `[`: x");
    }
}
