//! The colour tokens of a theme as the theme editor lists them: every
//! `[ui]`, `[syntax]` and `[terminal]` key, grouped by what it colours.

use super::{ANSI_NAMES, Color, TerminalColors, Theme};

/// The table of a theme file a token lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TokenSection {
    /// `[ui]`
    Ui,
    /// `[syntax]` (a tree-sitter capture name)
    Syntax,
    /// `[terminal]`
    Terminal,
}

/// One colour of a theme: its table and key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokenRef {
    /// The table.
    pub section: TokenSection,
    /// The key within the table (`text_muted`, `function.method`, `bright_red`).
    pub key: String,
}

impl TokenRef {
    /// A `[ui]` key.
    pub fn ui(key: &str) -> Self {
        Self::new(TokenSection::Ui, key)
    }

    /// A `[syntax]` capture.
    pub fn syntax(key: &str) -> Self {
        Self::new(TokenSection::Syntax, key)
    }

    /// A `[terminal]` key.
    pub fn terminal(key: &str) -> Self {
        Self::new(TokenSection::Terminal, key)
    }

    fn new(section: TokenSection, key: &str) -> Self {
        Self {
            section,
            key: key.to_owned(),
        }
    }

    /// `ui.text`, `syntax.keyword`, `terminal.red`: how the token is named
    /// in messages.
    pub fn path(&self) -> String {
        let table = match self.section {
            TokenSection::Ui => "ui",
            TokenSection::Syntax => "syntax",
            TokenSection::Terminal => "terminal",
        };
        format!("{table}.{}", self.key)
    }
}

/// A heading of the editor's list and the tokens under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGroup {
    /// The heading.
    pub title: &'static str,
    /// The tokens, in display order.
    pub tokens: Vec<TokenRef>,
}

/// `[ui]` keys by group. Together they hold every key of [`UiColors`].
const UI_GROUPS: [(&str, &[&str]); 5] = [
    (
        "Surfaces",
        &[
            "canvas",
            "background",
            "surface",
            "surface_raised",
            "pill",
            "hover",
            "line_highlight",
            "selection",
            "selection_inactive",
            "paused_line",
            "scrollbar_thumb",
        ],
    ),
    ("Borders", &["border", "border_strong", "border_focused"]),
    (
        "Text and accent",
        &[
            "text",
            "text_muted",
            "text_disabled",
            "accent",
            "accent_text",
            "accent_alt",
            "cursor",
            "gutter_text",
            "gutter_text_active",
        ],
    ),
    (
        "Git and diagnostics",
        &[
            "diff_added",
            "diff_modified",
            "diff_deleted",
            "error",
            "warning",
            "info",
            "hint",
            "success",
        ],
    ),
    (
        "Classic keys",
        &[
            "title_bar",
            "status_bar",
            "activity_bar",
            "panel_header",
            "tab_active",
            "tab_inactive",
        ],
    ),
];

/// The order syntax captures are listed and written in (the order of the
/// built-in theme files); other captures follow alphabetically.
pub const SYNTAX_ORDER: [&str; 33] = [
    "keyword",
    "function",
    "function.method",
    "function.builtin",
    "function.macro",
    "type",
    "type.builtin",
    "constructor",
    "string",
    "character",
    "string.special",
    "string.escape",
    "escape",
    "number",
    "boolean",
    "constant",
    "constant.builtin",
    "comment",
    "comment.doc",
    "variable",
    "variable.parameter",
    "variable.builtin",
    "property",
    "attribute",
    "tag.attribute",
    "label",
    "module",
    "namespace",
    "punctuation",
    "punctuation.delimiter",
    "operator",
    "punctuation.bracket",
    "tag",
];

/// The capture names of `syntax` in [`SYNTAX_ORDER`], then the others
/// alphabetically.
pub fn ordered_captures<'a>(syntax: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut known: Vec<(usize, String)> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for name in syntax {
        match SYNTAX_ORDER.iter().position(|n| n == name) {
            Some(ix) => known.push((ix, name.clone())),
            None => other.push(name.clone()),
        }
    }
    known.sort();
    other.sort();
    known.into_iter().map(|(_, n)| n).chain(other).collect()
}

/// The groups of the editor's list for `theme`: the `[ui]` groups, the
/// theme's syntax captures and the terminal colours.
pub fn token_groups(theme: &Theme) -> Vec<TokenGroup> {
    let mut groups: Vec<TokenGroup> = UI_GROUPS
        .iter()
        .map(|(title, keys)| TokenGroup {
            title,
            tokens: keys.iter().map(|k| TokenRef::ui(k)).collect(),
        })
        .collect();
    let syntax = TokenGroup {
        title: "Syntax",
        tokens: ordered_captures(theme.syntax.keys())
            .iter()
            .map(|k| TokenRef::syntax(k))
            .collect(),
    };
    // Syntax right after the interface colours, before git and the
    // classic keys.
    groups.insert(3, syntax);
    groups.insert(
        5,
        TokenGroup {
            title: "Terminal",
            tokens: TerminalColors::FIELDS
                .iter()
                .map(|k| TokenRef::terminal(k))
                .collect(),
        },
    );
    groups
}

/// What a `[ui]` key colours, in a few words (empty for other keys).
pub fn ui_description(key: &str) -> &'static str {
    match key {
        "canvas" => "Window background behind the panels",
        "background" => "Panels, the editor and the terminal",
        "surface" => "Classic side bars",
        "surface_raised" => "Palette, menus, popovers, dialogs",
        "pill" => "Active tab, selected rows, badges",
        "hover" => "Row and button hover",
        "line_highlight" => "Cursor line",
        "selection" => "Selected text",
        "selection_inactive" => "Selection in an unfocused editor",
        "paused_line" => "Debugger: the paused line",
        "scrollbar_thumb" => "Scrollbar thumb",
        "border" => "Panel edges and dividers",
        "border_strong" => "Control outlines and overlay edges",
        "border_focused" => "Edge of the focused panel",
        "text" => "Primary text",
        "text_muted" => "Secondary text, inactive tabs",
        "text_disabled" => "Disabled controls",
        "accent" => "Ember mark, links, the primary button",
        "accent_text" => "Text on an accent fill",
        "accent_alt" => "Unsaved ring, \"changed\" tags",
        "cursor" => "Caret",
        "gutter_text" => "Line numbers",
        "gutter_text_active" => "Line number of the cursor line",
        "diff_added" => "Added lines (bars)",
        "diff_modified" => "Modified lines (bars)",
        "diff_deleted" => "Deleted lines (triangles)",
        "error" => "Errors and breakpoints",
        "warning" => "Warnings",
        "info" => "Information, modified files",
        "hint" => "Hints",
        "success" => "Success, added files",
        "title_bar" => "Classic title bar",
        "status_bar" => "Classic status bar",
        "activity_bar" => "Classic activity bar",
        "panel_header" => "Classic panel headers",
        "tab_active" => "Classic active tab",
        "tab_inactive" => "Classic inactive tab",
        _ => "",
    }
}

/// The ANSI index of a `[terminal]` key, for labels such as "ANSI 9".
pub fn ansi_index(key: &str) -> Option<usize> {
    ANSI_NAMES.iter().position(|n| *n == key)
}

impl Theme {
    /// The colour of `token` as drawn: a syntax capture without a colour
    /// of its own is drawn in `ui.text`. `None` for an unknown key.
    pub fn token_color(&self, token: &TokenRef) -> Option<Color> {
        match token.section {
            TokenSection::Ui => self.ui.get(&token.key),
            TokenSection::Syntax => self
                .syntax
                .get(&token.key)
                .map(|style| style.color.unwrap_or(self.ui.text)),
            TokenSection::Terminal => self.terminal.get(&token.key),
        }
    }

    /// Sets the colour of `token`. A syntax capture keeps its bold, italic
    /// and underline flags; a capture the theme lacks is added. Returns
    /// `false` for an unknown `[ui]` or `[terminal]` key.
    pub fn set_token_color(&mut self, token: &TokenRef, color: Color) -> bool {
        match token.section {
            TokenSection::Ui => self.ui.set(&token.key, color),
            TokenSection::Terminal => self.terminal.set(&token.key, color),
            TokenSection::Syntax => {
                let style = self.syntax.entry(token.key.clone()).or_default();
                style.color = Some(color);
                true
            }
        }
    }
}

/// Every `[ui]` key the groups list, in order (tests).
#[cfg(test)]
pub(super) fn grouped_ui_keys() -> Vec<&'static str> {
    UI_GROUPS
        .iter()
        .flat_map(|(_, keys)| keys.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{SyntaxStyle, UiColors};
    use super::*;
    use crate::ThemeRegistry;

    #[test]
    fn groups_hold_every_ui_key_once() {
        let mut keys = grouped_ui_keys();
        assert_eq!(keys.len(), UiColors::FIELDS.len());
        keys.sort();
        let mut fields = UiColors::FIELDS.to_vec();
        fields.sort();
        assert_eq!(keys, fields);
        for key in UiColors::FIELDS {
            assert!(!ui_description(key).is_empty(), "{key}");
        }
    }

    #[test]
    fn groups_list_syntax_and_terminal() {
        let registry = ThemeRegistry::builtin();
        let ember = registry.get("Arcode Ember").unwrap();
        let groups = token_groups(ember);
        let titles: Vec<_> = groups.iter().map(|g| g.title).collect();
        assert_eq!(
            titles,
            [
                "Surfaces",
                "Borders",
                "Text and accent",
                "Syntax",
                "Git and diagnostics",
                "Terminal",
                "Classic keys"
            ]
        );
        let syntax = &groups[3].tokens;
        assert_eq!(syntax.len(), ember.syntax.len());
        assert_eq!(syntax[0], TokenRef::syntax("keyword"));
        assert_eq!(groups[5].tokens.len(), 19);
        assert_eq!(ansi_index("bright_red"), Some(9));
        assert_eq!(TokenRef::terminal("red").path(), "terminal.red");
    }

    #[test]
    fn unknown_captures_follow_the_known_order() {
        let names: Vec<String> = ["zeta", "keyword", "alpha", "tag", "function"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            ordered_captures(&names),
            ["keyword", "function", "tag", "alpha", "zeta"]
        );
    }

    #[test]
    fn colours_are_read_and_set_by_token() {
        let mut theme = ThemeRegistry::builtin()
            .get("Arcode Chalk")
            .unwrap()
            .clone();
        let red = Color::rgb(255, 0, 0);
        for token in [
            TokenRef::ui("accent"),
            TokenRef::syntax("comment"),
            TokenRef::terminal("bright_blue"),
        ] {
            assert!(theme.set_token_color(&token, red), "{token:?}");
            assert_eq!(theme.token_color(&token), Some(red));
        }
        // The comment stays italic.
        assert!(theme.syntax["comment"].italic);
        assert!(!theme.set_token_color(&TokenRef::ui("nope"), red));
        assert_eq!(theme.token_color(&TokenRef::terminal("nope")), None);
        // A capture without a colour is drawn in the text colour.
        theme.syntax.insert("x".into(), SyntaxStyle::default());
        assert_eq!(
            theme.token_color(&TokenRef::syntax("x")),
            Some(theme.ui.text)
        );
        assert!(theme.set_token_color(&TokenRef::syntax("new.capture"), red));
        assert_eq!(theme.syntax_color("new.capture"), Some(red));
    }
}
