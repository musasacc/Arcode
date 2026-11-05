//! The state of the theme editor: the theme being edited, the colours it
//! started from, which derivable `[ui]` keys still follow their sources,
//! and the contrast of the pairs that matter for reading.

use std::collections::BTreeSet;

use super::derive::{derived_keys, derived_value};
use super::save::is_builtin_name;
use super::tokens::{TokenRef, TokenSection};
use super::{Color, Theme};

/// A theme being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeEdit {
    /// The theme as editing started (or as last saved).
    start: Theme,
    start_derived: BTreeSet<&'static str>,
    /// The theme with the edits.
    working: Theme,
    /// Derivable `[ui]` keys that follow their source keys.
    derived: BTreeSet<&'static str>,
}

impl ThemeEdit {
    /// Starts editing `theme`. A derivable key whose colour is what its
    /// sources give is shown as derived and keeps following them.
    pub fn new(theme: Theme) -> Self {
        let derived: BTreeSet<&'static str> = derived_keys()
            .filter(|key| derived_value(&theme.ui, key) == theme.ui.get(key))
            .collect();
        Self {
            start_derived: derived.clone(),
            start: theme.clone(),
            working: theme,
            derived,
        }
    }

    /// The theme with the edits.
    pub fn working(&self) -> &Theme {
        &self.working
    }

    /// The theme as editing started (or as last saved).
    pub fn start(&self) -> &Theme {
        &self.start
    }

    /// Whether the edited theme is a built-in one (saved only as a copy).
    pub fn is_builtin(&self) -> bool {
        is_builtin_name(&self.start.name)
    }

    /// Whether anything changed since editing started or the last save.
    pub fn is_dirty(&self) -> bool {
        self.working != self.start || self.derived != self.start_derived
    }

    /// Whether `token` differs from the colour editing started from.
    pub fn is_modified(&self, token: &TokenRef) -> bool {
        self.working.token_color(token) != self.start.token_color(token)
    }

    /// Whether the `[ui]` key `key` follows its sources.
    pub fn is_derived(&self, key: &str) -> bool {
        self.derived.contains(key)
    }

    /// Sets a colour. Setting a derived key overrides it; derived keys
    /// follow a changed source. Returns whether the theme changed.
    pub fn set(&mut self, token: &TokenRef, color: Color) -> bool {
        let before = self.working.clone();
        if token.section == TokenSection::Ui {
            self.derived.remove(token.key.as_str());
        }
        if !self.working.set_token_color(token, color) {
            return false;
        }
        self.refresh_derived();
        self.working != before
    }

    /// Stops `key` following its sources; its colour stays as it is.
    pub fn override_derived(&mut self, key: &str) {
        self.derived.remove(key);
    }

    /// Makes the derivable `key` follow its sources again.
    pub fn follow_derived(&mut self, key: &str) {
        if let Some(key) = derived_keys().find(|k| *k == key) {
            self.derived.insert(key);
            self.refresh_derived();
        }
    }

    fn refresh_derived(&mut self) {
        for key in &self.derived {
            if let Some(color) = derived_value(&self.working.ui, key) {
                self.working.ui.set(key, color);
            }
        }
    }

    /// Drops every edit.
    pub fn discard(&mut self) {
        self.working = self.start.clone();
        self.derived = self.start_derived.clone();
    }

    /// The `[ui]` keys a saved file leaves out (they are derived on load).
    pub fn omitted_keys(&self) -> Vec<&'static str> {
        self.derived.iter().copied().collect()
    }

    /// The edited theme as a theme file (see [`Theme::to_toml_string`]).
    pub fn to_toml(&self) -> String {
        self.working.to_toml_string(&self.omitted_keys())
    }

    /// The working theme renamed to `name` (what "Save as" writes).
    pub fn renamed(&self, name: &str) -> Theme {
        Theme {
            name: name.to_owned(),
            ..self.working.clone()
        }
    }

    /// Records a save as `name`: the saved colours become the new start,
    /// and later saves go to that (user) theme.
    pub fn saved_as(&mut self, name: &str) {
        self.working.name = name.to_owned();
        self.start = self.working.clone();
        self.start_derived = self.derived.clone();
    }

    /// The contrast of the pairs [`contrast_hints`] checks, for the
    /// working theme.
    pub fn contrast_hints(&self) -> Vec<ContrastHint> {
        contrast_hints(&self.working)
    }
}

/// WCAG minimum for body text.
pub const TEXT_CONTRAST: f32 = 4.5;
/// WCAG minimum for large text and UI elements (outlines, the caret).
pub const UI_CONTRAST: f32 = 3.0;

/// The contrast of a foreground on a background colour of a theme.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastHint {
    /// What the pair is ("Muted text on panels").
    pub label: &'static str,
    /// The foreground token.
    pub fg: TokenRef,
    /// The background token.
    pub bg: TokenRef,
    /// The WCAG contrast ratio (translucent colours composited first).
    pub ratio: f32,
    /// The minimum it should reach ([`TEXT_CONTRAST`] or [`UI_CONTRAST`]).
    pub min: f32,
}

impl ContrastHint {
    /// Whether the ratio reaches the minimum.
    pub fn passes(&self) -> bool {
        self.ratio >= self.min
    }
}

/// `(label, foreground, background, minimum)`; tokens as `table.key`.
const PAIRS: [(&str, &str, &str, f32); 12] = [
    ("Text on panels", "ui.text", "ui.background", TEXT_CONTRAST),
    (
        "Muted text on panels",
        "ui.text_muted",
        "ui.background",
        TEXT_CONTRAST,
    ),
    (
        "Text on overlays",
        "ui.text",
        "ui.surface_raised",
        TEXT_CONTRAST,
    ),
    ("Selected text", "ui.text", "ui.selection", TEXT_CONTRAST),
    (
        "Text on the cursor line",
        "ui.text",
        "ui.line_highlight",
        TEXT_CONTRAST,
    ),
    (
        "Line numbers",
        "ui.gutter_text",
        "ui.background",
        TEXT_CONTRAST,
    ),
    (
        "Text on accent",
        "ui.accent_text",
        "ui.accent",
        TEXT_CONTRAST,
    ),
    ("Comments", "syntax.comment", "ui.background", TEXT_CONTRAST),
    (
        "Terminal text",
        "terminal.foreground",
        "terminal.background",
        TEXT_CONTRAST,
    ),
    (
        "Accent on panels",
        "ui.accent",
        "ui.background",
        UI_CONTRAST,
    ),
    (
        "Control outlines",
        "ui.border_strong",
        "ui.background",
        UI_CONTRAST,
    ),
    (
        "Caret on the cursor line",
        "ui.cursor",
        "ui.line_highlight",
        UI_CONTRAST,
    ),
];

fn token(path: &str) -> TokenRef {
    match path.split_once('.') {
        Some(("ui", key)) => TokenRef::ui(key),
        Some(("syntax", key)) => TokenRef::syntax(key),
        Some((_, key)) => TokenRef::terminal(key),
        None => TokenRef::ui(path),
    }
}

/// The contrast of the text-on-background pairs that matter most. A
/// translucent background is composited over the panel (the terminal
/// background for terminal pairs), a translucent foreground over that.
/// Pairs naming a capture the theme lacks are left out.
pub fn contrast_hints(theme: &Theme) -> Vec<ContrastHint> {
    let panel = theme.ui.background.with_alpha(255);
    PAIRS
        .iter()
        .filter_map(|(label, fg, bg, min)| {
            let (fg, bg) = (token(fg), token(bg));
            let ground = if bg.section == TokenSection::Terminal {
                theme.terminal.background.with_alpha(255)
            } else {
                panel
            };
            let back = theme.token_color(&bg)?.over(ground);
            let fore = theme.token_color(&fg)?.over(back);
            Some(ContrastHint {
                label,
                ratio: fore.contrast_ratio(back),
                fg,
                bg,
                min: *min,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::BUILTIN_THEME_NAMES;
    use super::*;
    use crate::ThemeRegistry;

    fn edit(name: &str) -> ThemeEdit {
        ThemeEdit::new(ThemeRegistry::builtin().get(name).unwrap().clone())
    }

    #[test]
    fn edits_mark_dirty_and_discard_restores() {
        let mut e = edit("Arcode Ember");
        assert!(e.is_builtin());
        assert!(!e.is_dirty());
        let accent = TokenRef::ui("accent");
        let keyword = TokenRef::syntax("keyword");
        let red = Color::rgb(220, 30, 30);
        assert!(e.set(&accent, red));
        assert!(!e.set(&accent, red), "same colour again changes nothing");
        assert!(e.set(&keyword, red));
        assert!(e.is_dirty());
        assert!(e.is_modified(&accent));
        assert!(!e.is_modified(&TokenRef::ui("text")));
        assert_eq!(e.working().ui.accent, red);
        assert_eq!(e.working().syntax_color("keyword"), Some(red));
        assert!(!e.set(&TokenRef::ui("nope"), red));
        e.discard();
        assert!(!e.is_dirty());
        assert_eq!(e.working(), e.start());
    }

    #[test]
    fn derived_keys_follow_until_overridden() {
        let mut e = edit("Arcode Ember");
        // Ember's canvas is its title bar colour; its pill is its own.
        assert!(e.is_derived("canvas"));
        assert!(!e.is_derived("pill"));
        let blue = Color::rgb(10, 20, 90);
        e.set(&TokenRef::ui("title_bar"), blue);
        assert_eq!(e.working().ui.canvas, blue);
        assert_eq!(e.omitted_keys(), ["canvas"]);
        // Setting the derived key itself overrides it.
        let green = Color::rgb(0, 90, 0);
        e.set(&TokenRef::ui("canvas"), green);
        assert!(!e.is_derived("canvas"));
        e.set(&TokenRef::ui("title_bar"), Color::BLACK);
        assert_eq!(e.working().ui.canvas, green);
        // ... until it follows again.
        e.follow_derived("canvas");
        assert_eq!(e.working().ui.canvas, Color::BLACK);
        e.override_derived("canvas");
        assert!(!e.is_derived("canvas"));
        assert!(e.is_dirty(), "following state is part of the edit");
        e.follow_derived("text");
        assert!(!e.is_derived("text"));
    }

    #[test]
    fn saving_moves_the_start() {
        let mut e = edit("Arcode Chalk");
        e.set(&TokenRef::ui("accent"), Color::rgb(1, 2, 3));
        assert_eq!(e.renamed("My Chalk").name, "My Chalk");
        e.saved_as("My Chalk");
        assert!(!e.is_dirty());
        assert!(!e.is_builtin());
        assert_eq!(e.start().name, "My Chalk");
        // The file parses back to the working theme.
        let (theme, warnings) = Theme::from_toml_str(&e.to_toml(), None).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(&theme, e.working());
    }

    #[test]
    fn contrast_of_the_builtins_passes() {
        for name in BUILTIN_THEME_NAMES {
            let hints = edit(name).contrast_hints();
            assert_eq!(hints.len(), PAIRS.len(), "{name}");
            if name == "Arcode Ember" || name == "Arcode Chalk" {
                for hint in &hints {
                    assert!(hint.passes(), "{name}: {hint:?}");
                }
            }
        }
    }

    #[test]
    fn low_contrast_is_reported() {
        let mut e = edit("Arcode Ember");
        let panel = e.working().ui.background;
        e.set(&TokenRef::ui("text_muted"), panel);
        let hint = e
            .contrast_hints()
            .into_iter()
            .find(|h| h.label == "Muted text on panels")
            .unwrap();
        assert!((hint.ratio - 1.0).abs() < 1e-3);
        assert!(!hint.passes());
        assert_eq!(hint.fg, TokenRef::ui("text_muted"));
        // A translucent selection is composited over the panel first.
        e.set(&TokenRef::ui("selection"), Color::rgba(255, 255, 255, 0));
        let selected = e
            .contrast_hints()
            .into_iter()
            .find(|h| h.label == "Selected text")
            .unwrap();
        let on_panel = e.working().ui.text.contrast_ratio(panel);
        assert!((selected.ratio - on_panel).abs() < 1e-3);
        // A theme without comments has no comment pair.
        let mut theme = e.working().clone();
        theme.syntax.remove("comment");
        assert!(contrast_hints(&theme).iter().all(|h| h.label != "Comments"));
    }
}
