//! The active theme as GPUI colors, stored in a GPUI global.
//!
//! Themes are authored as TOML (`arcode_config::theme`); this module converts
//! the active one once, so views read ready-to-use [`Hsla`] values.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use arcode_config::theme::{Appearance, Color, Theme};
use gpui::{App, FontStyle, FontWeight, Global, HighlightStyle, Hsla, Rgba, UnderlineStyle, px};

/// Converts a config color into a GPUI color.
pub fn hsla(c: Color) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.,
        g: c.g as f32 / 255.,
        b: c.b as f32 / 255.,
        a: c.a as f32 / 255.,
    }
    .into()
}

/// UI colors, one field per `[ui]` key in a theme file.
#[derive(Clone, Debug)]
pub struct ThemeColors {
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub border: Hsla,
    pub border_focused: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_disabled: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub accent_alt: Hsla,
    pub selection: Hsla,
    pub cursor: Hsla,
    pub line_highlight: Hsla,
    pub gutter_text: Hsla,
    pub gutter_text_active: Hsla,
    pub error: Hsla,
    pub warning: Hsla,
    pub info: Hsla,
    pub hint: Hsla,
    pub success: Hsla,
    pub scrollbar_thumb: Hsla,
    pub tab_active: Hsla,
    pub tab_inactive: Hsla,
    pub title_bar: Hsla,
    pub status_bar: Hsla,
    pub activity_bar: Hsla,
    pub panel_header: Hsla,
    pub hover: Hsla,
    pub diff_added: Hsla,
    pub diff_modified: Hsla,
    pub diff_deleted: Hsla,
}

/// Terminal palette in GPUI colors.
#[derive(Clone, Debug)]
pub struct TerminalColors {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub ansi: [Hsla; 16],
}

/// A theme ready for drawing.
pub struct UiTheme {
    pub name: String,
    pub is_dark: bool,
    pub colors: ThemeColors,
    pub terminal: TerminalColors,
    /// The source theme, e.g. for the terminal palette in `u8` form.
    pub source: Arc<Theme>,
    syntax_cache: RefCell<HashMap<String, HighlightStyle>>,
}

impl UiTheme {
    pub fn new(theme: Arc<Theme>) -> Self {
        let u = &theme.ui;
        let colors = ThemeColors {
            background: hsla(u.background),
            surface: hsla(u.surface),
            surface_raised: hsla(u.surface_raised),
            border: hsla(u.border),
            border_focused: hsla(u.border_focused),
            text: hsla(u.text),
            text_muted: hsla(u.text_muted),
            text_disabled: hsla(u.text_disabled),
            accent: hsla(u.accent),
            accent_text: hsla(u.accent_text),
            accent_alt: hsla(u.accent_alt),
            selection: hsla(u.selection),
            cursor: hsla(u.cursor),
            line_highlight: hsla(u.line_highlight),
            gutter_text: hsla(u.gutter_text),
            gutter_text_active: hsla(u.gutter_text_active),
            error: hsla(u.error),
            warning: hsla(u.warning),
            info: hsla(u.info),
            hint: hsla(u.hint),
            success: hsla(u.success),
            scrollbar_thumb: hsla(u.scrollbar_thumb),
            tab_active: hsla(u.tab_active),
            tab_inactive: hsla(u.tab_inactive),
            title_bar: hsla(u.title_bar),
            status_bar: hsla(u.status_bar),
            activity_bar: hsla(u.activity_bar),
            panel_header: hsla(u.panel_header),
            hover: hsla(u.hover),
            diff_added: hsla(u.diff_added),
            diff_modified: hsla(u.diff_modified),
            diff_deleted: hsla(u.diff_deleted),
        };
        let t = &theme.terminal;
        let terminal = TerminalColors {
            foreground: hsla(t.foreground),
            background: hsla(t.background),
            cursor: hsla(t.cursor),
            ansi: t.ansi.map(hsla),
        };
        Self {
            name: theme.name.clone(),
            is_dark: theme.appearance == Appearance::Dark,
            colors,
            terminal,
            source: theme,
            syntax_cache: RefCell::default(),
        }
    }

    /// Style for a tree-sitter capture name such as `function.method`, with
    /// dotted-prefix fallback. Unknown captures get the default text color.
    pub fn syntax(&self, capture: &str) -> HighlightStyle {
        if let Some(style) = self.syntax_cache.borrow().get(capture) {
            return *style;
        }
        let style = match self.source.syntax_style(capture) {
            Some(s) => HighlightStyle {
                color: Some(s.color.map(hsla).unwrap_or(self.colors.text)),
                font_weight: s.bold.then_some(FontWeight::BOLD),
                font_style: s.italic.then_some(FontStyle::Italic),
                underline: s.underline.then_some(UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            },
            None => HighlightStyle {
                color: Some(self.colors.text),
                ..Default::default()
            },
        };
        self.syntax_cache
            .borrow_mut()
            .insert(capture.to_string(), style);
        style
    }
}

/// Global holding the active theme.
pub struct ActiveTheme(pub Rc<UiTheme>);

impl Global for ActiveTheme {}

/// The active theme. Panics if [`set_theme`] was never called.
pub fn theme(cx: &App) -> &UiTheme {
    &cx.global::<ActiveTheme>().0
}

/// Makes `theme` the active theme and refreshes every window.
pub fn set_theme(theme: Arc<Theme>, cx: &mut App) {
    cx.set_global(ActiveTheme(Rc::new(UiTheme::new(theme))));
    cx.refresh_windows();
}
