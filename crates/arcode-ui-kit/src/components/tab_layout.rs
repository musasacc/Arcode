//! Sizes and the trailing-slot rule of pill tabs (docs/DESIGN.md 6.6,
//! 6.8), kept apart from the element so the numbers are easy to test.

use gpui::{Pixels, px};

use crate::metrics::{TAB_H, TAB_H_SMALL, TypeScale};

/// The three tab sizes of docs/DESIGN.md 6.6 and 6.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSize {
    /// Editor tabs: 26 tall, label 13, icon 15, padding 0 8 0 10.
    #[default]
    Editor,
    /// Bottom panel tabs (Terminal, Problems, Todo, Debug): 26 tall,
    /// label 13, icon 14, padding 0 9.
    Panel,
    /// Terminal session pills: 24 tall, label 12, icon 13, padding 0 9;
    /// the active session is `hover` filled, not `pill`.
    Session,
}

/// Height, label size, icon size and left padding of a tab size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabMetrics {
    /// Tab height.
    pub height: Pixels,
    /// Label text size.
    pub text: Pixels,
    /// Leading icon size.
    pub icon: Pixels,
    /// Left padding (and right padding without a trailing slot).
    pub pad: Pixels,
}

impl TabSize {
    /// The metrics of this size under `scale`.
    pub fn metrics(self, scale: &TypeScale) -> TabMetrics {
        let (height, text, icon, pad) = match self {
            Self::Editor => (TAB_H, scale.body, 15., 10.),
            Self::Panel => (TAB_H, scale.body, 14., 9.),
            Self::Session => (TAB_H_SMALL, scale.meta, 13., 9.),
        };
        TabMetrics {
            height,
            text,
            icon: px(icon),
            pad: px(pad),
        }
    }
}

/// What the trailing slot of a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabTrailing {
    /// Nothing.
    None,
    /// The close `x`, always visible.
    Close,
    /// The close `x`, visible only on hover.
    CloseOnHover,
    /// The unsaved ring; the close `x` replaces it on hover when closable.
    Ring { closable: bool },
}

/// The trailing slot for a tab's state (6.6): unsaved shows the ring,
/// the active tab shows `x`, inactive tabs show `x` on hover.
pub fn tab_trailing(active: bool, dirty: bool, closable: bool) -> TabTrailing {
    match (dirty, closable, active) {
        (true, _, _) => TabTrailing::Ring { closable },
        (false, false, _) => TabTrailing::None,
        (false, true, true) => TabTrailing::Close,
        (false, true, false) => TabTrailing::CloseOnHover,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_follows_the_tab_state() {
        assert_eq!(tab_trailing(true, false, true), TabTrailing::Close);
        assert_eq!(tab_trailing(false, false, true), TabTrailing::CloseOnHover);
        assert_eq!(tab_trailing(true, false, false), TabTrailing::None);
        assert_eq!(
            tab_trailing(true, true, true),
            TabTrailing::Ring { closable: true }
        );
        assert_eq!(
            tab_trailing(false, true, false),
            TabTrailing::Ring { closable: false }
        );
    }
}
