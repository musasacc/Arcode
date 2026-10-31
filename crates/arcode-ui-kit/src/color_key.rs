//! Looking up theme colors by their `[ui]` key name.
//!
//! Extension points (editor decorations, plugins) refer to colors by key,
//! e.g. `"diff_added"` or `"warning"`, so they never hard-code a color and
//! follow theme switches automatically.

use gpui::{Hsla, SharedString};

use crate::ThemeColors;

/// A theme color referenced by its `[ui]` key, with an optional opacity
/// multiplier (`1.0` keeps the theme's alpha).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorKey {
    /// Key such as `"accent"`, `"diff_added"` or `"error"`.
    pub key: SharedString,
    /// Multiplier applied to the color's alpha.
    pub opacity: f32,
}

impl ColorKey {
    /// The color `key` at full theme opacity.
    pub fn new(key: impl Into<SharedString>) -> Self {
        ColorKey {
            key: key.into(),
            opacity: 1.0,
        }
    }

    /// The same key with its alpha multiplied by `opacity`.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Resolves against `colors`; unknown keys fall back to `text_muted`.
    pub fn resolve(&self, colors: &ThemeColors) -> Hsla {
        let mut color = colors.get(&self.key).unwrap_or(colors.text_muted);
        color.a *= self.opacity;
        color
    }
}

impl From<&'static str> for ColorKey {
    fn from(key: &'static str) -> Self {
        ColorKey::new(key)
    }
}

impl ThemeColors {
    /// The color for a `[ui]` key name, or `None` for an unknown key.
    pub fn get(&self, key: &str) -> Option<Hsla> {
        Some(match key {
            "background" => self.background,
            "surface" => self.surface,
            "surface_raised" => self.surface_raised,
            "border" => self.border,
            "border_focused" => self.border_focused,
            "text" => self.text,
            "text_muted" => self.text_muted,
            "text_disabled" => self.text_disabled,
            "accent" => self.accent,
            "accent_text" => self.accent_text,
            "accent_alt" => self.accent_alt,
            "selection" => self.selection,
            "cursor" => self.cursor,
            "line_highlight" => self.line_highlight,
            "gutter_text" => self.gutter_text,
            "gutter_text_active" => self.gutter_text_active,
            "error" => self.error,
            "warning" => self.warning,
            "info" => self.info,
            "hint" => self.hint,
            "success" => self.success,
            "scrollbar_thumb" => self.scrollbar_thumb,
            "tab_active" => self.tab_active,
            "tab_inactive" => self.tab_inactive,
            "title_bar" => self.title_bar,
            "status_bar" => self.status_bar,
            "activity_bar" => self.activity_bar,
            "panel_header" => self.panel_header,
            "hover" => self.hover,
            "diff_added" => self.diff_added,
            "diff_modified" => self.diff_modified,
            "diff_deleted" => self.diff_deleted,
            "canvas" => self.canvas,
            "pill" => self.pill,
            "border_strong" => self.border_strong,
            "selection_inactive" => self.selection_inactive,
            "paused_line" => self.paused_line,
            "shadow" => self.shadow,
            "accent_hover" => self.accent_hover,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UiTheme;

    fn colors() -> ThemeColors {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        UiTheme::new(std::sync::Arc::new(theme)).colors
    }

    #[test]
    fn keys_resolve() {
        let c = colors();
        assert_eq!(c.get("diff_added"), Some(c.diff_added));
        assert_eq!(c.get("warning"), Some(c.warning));
        assert_eq!(c.get("nope"), None);
        assert_eq!(c.get("canvas"), Some(c.canvas));
        assert_eq!(c.get("paused_line"), Some(c.paused_line));
        // Every `[ui]` key of the theme format resolves.
        for key in arcode_config::UiColors::FIELDS {
            assert!(c.get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn opacity_and_fallback() {
        let c = colors();
        let half = ColorKey::new("error").opacity(0.5).resolve(&c);
        assert!((half.a - c.error.a * 0.5).abs() < 1e-6);
        assert_eq!(ColorKey::new("unknown").resolve(&c), c.text_muted);
    }
}
