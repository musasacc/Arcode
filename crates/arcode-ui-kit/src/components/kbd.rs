//! Keyboard shortcut chips (docs/DESIGN.md 6.13): one chip per key.

use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::metrics::{WEIGHT_MEDIUM, type_scale};
use crate::theme;

/// A keyboard shortcut such as `Ctrl+Shift+P`, drawn as one chip per key
/// (18 tall, radius 5, `panel` fill, 1 px `border_strong`, micro Medium
/// muted). Chords (`Ctrl+K Ctrl+S`) get a wider gap between them.
#[derive(IntoElement)]
pub struct Kbd(SharedString);

/// Chips for a shortcut label (`"Ctrl+P"`, `"Shift+F11"`, `"Esc"`).
pub fn kbd(keys: impl Into<SharedString>) -> Kbd {
    Kbd(keys.into())
}

/// Splits a shortcut label into chords and each chord into its keys:
/// `"Ctrl+K Ctrl+S"` gives `[["Ctrl", "K"], ["Ctrl", "S"]]`. A `+` key is
/// kept (`"Ctrl++"` gives `["Ctrl", "+"]`); empty input gives nothing.
pub fn kbd_keys(label: &str) -> Vec<Vec<String>> {
    label
        .split_whitespace()
        .map(|chord| {
            let mut keys = Vec::new();
            let mut rest = chord;
            while !rest.is_empty() {
                // A leading `+` is the plus key itself, not a separator.
                let first = rest.chars().next().map_or(1, char::len_utf8);
                let end = rest[first..].find('+').map_or(rest.len(), |i| i + first);
                keys.push(rest[..end].to_owned());
                rest = rest[end..].strip_prefix('+').unwrap_or(&rest[end..]);
            }
            keys
        })
        .filter(|keys| !keys.is_empty())
        .collect()
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let size = type_scale(cx).micro;
        let chip = |key: String| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .h(px(18.))
                .min_w(px(18.))
                .px(px(5.))
                .rounded(px(5.))
                .bg(c.panel())
                .border_1()
                .border_color(c.border_strong)
                .text_size(size)
                .font_weight(WEIGHT_MEDIUM)
                .text_color(c.text_muted)
                .child(key)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .children(kbd_keys(&self.0).into_iter().map(|chord| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .children(chord.into_iter().map(chip))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_keys_and_chords() {
        assert_eq!(kbd_keys("Ctrl+Shift+P"), [["Ctrl", "Shift", "P"]]);
        assert_eq!(kbd_keys("Esc"), [["Esc"]]);
        assert_eq!(kbd_keys("Ctrl++"), [["Ctrl", "+"]]);
        assert_eq!(kbd_keys("+"), [["+"]]);
        assert_eq!(kbd_keys("Ctrl+K Ctrl+S"), [["Ctrl", "K"], ["Ctrl", "S"]]);
        assert_eq!(kbd_keys("Shift+`"), [["Shift", "`"]]);
        assert_eq!(kbd_keys("⌘+⇧+P"), [["⌘", "⇧", "P"]]);
        assert!(kbd_keys("  ").is_empty());
    }
}
