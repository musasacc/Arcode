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
    /// Window background; title bar, rail and footer sit on it unfilled.
    pub canvas: Hsla,
    /// Active tab pill, selected rows, the active rail tile, count badges.
    pub pill: Hsla,
    /// Control outlines (inputs, checkboxes, switches, kbd) and overlay edges.
    pub border_strong: Hsla,
    /// Text selection in an unfocused editor or terminal.
    pub selection_inactive: Hsla,
    /// Debugger: band behind the paused line, the Paused chip.
    pub paused_line: Hsla,
    /// Overlay shadow color (not a theme key): black at 45 % in dark
    /// themes, a warm brown at 16 % in light ones.
    pub shadow: Hsla,
    /// Solid hover fill of a Primary (accent) button, precomputed from
    /// `accent` (not a theme key).
    pub accent_hover: Hsla,
    /// File icon tints by kind (docs/DESIGN.md 6.5), taken from the syntax
    /// colors (not theme keys): source files `type`, manifests
    /// `function.macro`, docs `number`.
    pub file_source: Hsla,
    pub file_manifest: Hsla,
    pub file_doc: Hsla,
}

impl ThemeColors {
    /// Fill of every floating panel, the editor and the terminal (the
    /// legacy `background` key).
    pub fn panel(&self) -> Hsla {
        self.background
    }

    /// Fill of overlays: palette, menus, popovers, dialogs, toasts (the
    /// legacy `surface_raised` key).
    pub fn panel_raised(&self) -> Hsla {
        self.surface_raised
    }
}

/// The overlay shadow color for a dark or light theme.
pub fn shadow_color(is_dark: bool) -> Hsla {
    if is_dark {
        hsla(Color::rgba(0, 0, 0, 115))
    } else {
        hsla(Color::rgba(40, 32, 20, 41))
    }
}

/// The solid hover fill of an accent-filled button: lighter in dark
/// themes, darker in light ones, same hue.
pub fn accent_hover(accent: Hsla, is_dark: bool) -> Hsla {
    if is_dark {
        Hsla {
            s: (accent.s + 0.04).min(1.),
            l: (accent.l + 0.07).min(1.),
            ..accent
        }
    } else {
        Hsla {
            l: (accent.l - 0.05).max(0.),
            ..accent
        }
    }
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
        let is_dark = theme.appearance == Appearance::Dark;
        let syntax = |capture: &str| hsla(theme.syntax_color(capture).unwrap_or(u.text));
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
            canvas: hsla(u.canvas),
            pill: hsla(u.pill),
            border_strong: hsla(u.border_strong),
            selection_inactive: hsla(u.selection_inactive),
            paused_line: hsla(u.paused_line),
            shadow: shadow_color(is_dark),
            accent_hover: accent_hover(hsla(u.accent), is_dark),
            file_source: syntax("type"),
            file_manifest: syntax("function.macro"),
            file_doc: syntax("number"),
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
            is_dark,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_theme(name: &str) -> UiTheme {
        let registry = arcode_config::ThemeRegistry::builtin();
        UiTheme::new(Arc::new(registry.get(name).unwrap().clone()))
    }

    /// `color` as 8-bit RGB.
    fn rgb(color: Hsla) -> [i32; 3] {
        let c = Rgba::from(color);
        [c.r, c.g, c.b].map(|v| (v * 255.).round() as i32)
    }

    fn near(a: [i32; 3], b: [i32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 3)
    }

    #[test]
    fn new_keys_and_aliases() {
        let ember = ui_theme("Arcode Ember");
        let c = &ember.colors;
        assert!(ember.is_dark);
        assert_eq!(c.panel(), c.background);
        assert_eq!(c.panel_raised(), c.surface_raised);
        assert_eq!(rgb(c.canvas), [0x0b, 0x0a, 0x09]);
        assert_eq!(rgb(c.pill), [0x3a, 0x32, 0x2a]);
        assert!((c.shadow.a - 0.45).abs() < 0.01);
        let chalk = ui_theme("Arcode Chalk");
        assert!((chalk.colors.shadow.a - 0.16).abs() < 0.01);
    }

    #[test]
    fn primary_hover_matches_the_design() {
        // DESIGN.md 6.13: Primary hover #f6b468 (Ember) / #74470f (Chalk).
        let ember = ui_theme("Arcode Ember").colors.accent_hover;
        assert!(near(rgb(ember), [0xf6, 0xb4, 0x68]), "{:?}", rgb(ember));
        let chalk = ui_theme("Arcode Chalk").colors.accent_hover;
        assert!(near(rgb(chalk), [0x74, 0x47, 0x0f]), "{:?}", rgb(chalk));
        assert_eq!(ui_theme("Arcode Chalk").colors.accent_hover.a, 1.);
    }
}
