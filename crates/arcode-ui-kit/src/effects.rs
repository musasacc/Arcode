//! Low-end mode helpers: shadows, debounce delays and the font list.
//!
//! Low-end mode (`ui.low_end_mode`) turns animations off (see
//! [`crate::animation_duration`]), draws popups flat (a border, no shadow)
//! and stretches background refresh delays.

use std::time::Duration;

use gpui::{App, BoxShadow, Styled, point, px};

use crate::theme::shadow_color;
use crate::{ActiveTheme, BUNDLED_MONO_FONT, Settings};

/// Whether low-end mode is on (off when no settings are installed, as in
/// some tests).
pub fn low_end(cx: &App) -> bool {
    cx.try_global::<Settings>()
        .is_some_and(|s| s.0.ui.low_end_mode)
}

/// The delay before a background refresh (diagnostics counts, git): the
/// base delay, or three times it in low-end mode.
pub fn debounce(base_ms: u64, cx: &App) -> Duration {
    let ms = match cx.try_global::<Settings>() {
        Some(s) => s.0.ui.debounce_ms(base_ms),
        None => base_ms,
    };
    Duration::from_millis(ms)
}

/// The one-layer overlay shadow: `offset_y` down, `blur` wide, in the
/// theme's `shadow` color (black in dark themes, warm brown in light ones).
fn overlay_shadow(offset_y: f32, blur: f32, cx: &App) -> Vec<BoxShadow> {
    let color = cx
        .try_global::<ActiveTheme>()
        .map_or_else(|| shadow_color(true), |t| t.0.colors.shadow);
    vec![BoxShadow {
        color,
        offset: point(px(0.), px(offset_y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
    }]
}

/// Shadows for floating surfaces that follow low-end mode. Panels never
/// have shadows; only overlays do, one layer each (cheap on integrated
/// GPUs), and none at all in low-end mode, where overlays keep their
/// `border_strong` edge so they stay distinct.
pub trait Elevated: Styled + Sized {
    /// Popovers, menus, tooltips, toasts: offset 8, blur 16.
    fn elevated(self, cx: &App) -> Self {
        if low_end(cx) {
            self
        } else {
            self.shadow(overlay_shadow(8., 16., cx))
        }
    }

    /// The palette and dialogs: offset 12, blur 24.
    fn elevated_lg(self, cx: &App) -> Self {
        if low_end(cx) {
            self
        } else {
            self.shadow(overlay_shadow(12., 24., cx))
        }
    }
}

impl<T: Styled> Elevated for T {}

/// The font families to offer in a font picker: the bundled monospace font
/// first, then the system fonts sorted case-insensitively, without
/// duplicates and without hidden names (those starting with a dot, such as
/// `.SystemUIFont`).
pub fn font_choices(system: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = system
        .into_iter()
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty() && !n.starts_with('.') && n != BUNDLED_MONO_FONT)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names.insert(0, BUNDLED_MONO_FONT.to_owned());
    names
}

/// Whether a family name looks like a monospace (code) font, by name only:
/// the platform text system does not report it.
pub fn looks_monospace(family: &str) -> bool {
    let lower = family.to_lowercase();
    [
        "mono",
        "code",
        "courier",
        "consol",
        "menlo",
        "monaco",
        "hack",
        "terminal",
        "fixed",
        "inconsolata",
        "iosevka",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_choices_put_the_bundled_font_first() {
        let list = font_choices(
            [
                "zeta Sans",
                ".SystemUIFont",
                "Alpha",
                "JetBrains Mono",
                "alpha",
                " ",
                "DejaVu Sans Mono",
            ]
            .map(String::from),
        );
        assert_eq!(
            list,
            ["JetBrains Mono", "Alpha", "DejaVu Sans Mono", "zeta Sans"]
        );
    }

    #[test]
    fn monospace_guess() {
        assert!(looks_monospace("DejaVu Sans Mono"));
        assert!(looks_monospace("Fira Code"));
        assert!(!looks_monospace("Noto Sans"));
    }
}
