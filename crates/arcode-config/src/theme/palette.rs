//! The UI and terminal color sets of a theme.

use super::Color;

/// Defines a struct of named colors plus key-based accessors.
macro_rules! color_set {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$fmeta:meta])* $field:ident ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: Color, )*
        }

        impl $name {
            /// Every TOML key, in declaration order.
            pub const FIELDS: &'static [&'static str] = &[$(stringify!($field)),*];

            /// The color for a TOML key.
            pub fn get(&self, key: &str) -> Option<Color> {
                match key {
                    $(stringify!($field) => Some(self.$field),)*
                    _ => None,
                }
            }

            /// Builds the set by asking `lookup` for every key; returns the
            /// first missing key on failure.
            pub(crate) fn from_lookup(
                mut lookup: impl FnMut(&str) -> Option<Color>,
            ) -> Result<Self, &'static str> {
                Ok(Self {
                    $( $field: lookup(stringify!($field)).ok_or(stringify!($field))?, )*
                })
            }
        }
    };
}

color_set! {
    /// Colors of the workbench chrome and editor surfaces (`[ui]` in a theme).
    UiColors {
        /// Editor and main background.
        background,
        /// Side bars and panels.
        surface,
        /// Popups, menus, dialogs.
        surface_raised,
        /// Separators and outlines.
        border,
        /// Outline of the focused element.
        border_focused,
        /// Primary text.
        text,
        /// Secondary text (descriptions, inactive labels).
        text_muted,
        /// Disabled controls.
        text_disabled,
        /// Primary accent (buttons, links, focus).
        accent,
        /// Text drawn on top of `accent`.
        accent_text,
        /// Secondary (warm) accent.
        accent_alt,
        /// Text selection (usually translucent).
        selection,
        /// Editor caret.
        cursor,
        /// Background of the cursor line.
        line_highlight,
        /// Line numbers.
        gutter_text,
        /// Line number of the cursor line.
        gutter_text_active,
        /// Error diagnostics.
        error,
        /// Warning diagnostics.
        warning,
        /// Information diagnostics.
        info,
        /// Hint diagnostics.
        hint,
        /// Success states.
        success,
        /// Scrollbar thumb.
        scrollbar_thumb,
        /// Active tab background.
        tab_active,
        /// Inactive tab background.
        tab_inactive,
        /// Window title bar.
        title_bar,
        /// Status bar.
        status_bar,
        /// Activity bar (left icon strip).
        activity_bar,
        /// Panel headers.
        panel_header,
        /// Hover highlight (usually translucent).
        hover,
        /// Added lines in diffs and the gutter.
        diff_added,
        /// Modified lines.
        diff_modified,
        /// Deleted lines.
        diff_deleted,
    }
}

/// TOML key names of the 16 ANSI colors, in index order.
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// Terminal colors (`[terminal]` in a theme). The 16 ANSI colors are written
/// in TOML with the names in [`ANSI_NAMES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalColors {
    /// Default text color.
    pub foreground: Color,
    /// Default background color.
    pub background: Color,
    /// Cursor color.
    pub cursor: Color,
    /// ANSI colors 0-15.
    pub ansi: [Color; 16],
}

impl TerminalColors {
    /// Every TOML key: `foreground`, `background`, `cursor`, then the ANSI names.
    pub const FIELDS: [&'static str; 19] = [
        "foreground",
        "background",
        "cursor",
        ANSI_NAMES[0],
        ANSI_NAMES[1],
        ANSI_NAMES[2],
        ANSI_NAMES[3],
        ANSI_NAMES[4],
        ANSI_NAMES[5],
        ANSI_NAMES[6],
        ANSI_NAMES[7],
        ANSI_NAMES[8],
        ANSI_NAMES[9],
        ANSI_NAMES[10],
        ANSI_NAMES[11],
        ANSI_NAMES[12],
        ANSI_NAMES[13],
        ANSI_NAMES[14],
        ANSI_NAMES[15],
    ];

    /// The color for a TOML key.
    pub fn get(&self, key: &str) -> Option<Color> {
        match key {
            "foreground" => Some(self.foreground),
            "background" => Some(self.background),
            "cursor" => Some(self.cursor),
            _ => ANSI_NAMES
                .iter()
                .position(|name| *name == key)
                .map(|i| self.ansi[i]),
        }
    }

    /// ANSI color by index (0-15); out-of-range indices return `None`.
    pub fn ansi_color(&self, index: usize) -> Option<Color> {
        self.ansi.get(index).copied()
    }

    pub(crate) fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<Color>,
    ) -> Result<Self, &'static str> {
        let mut need = |key: &'static str| lookup(key).ok_or(key);
        let foreground = need("foreground")?;
        let background = need("background")?;
        let cursor = need("cursor")?;
        let mut ansi = [Color::BLACK; 16];
        for (slot, name) in ansi.iter_mut().zip(ANSI_NAMES) {
            *slot = need(name)?;
        }
        Ok(Self {
            foreground,
            background,
            cursor,
            ansi,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_fields_are_complete() {
        assert_eq!(UiColors::FIELDS.len(), 32);
        assert!(UiColors::FIELDS.contains(&"accent_text"));
        let ui = UiColors::from_lookup(|_| Some(Color::WHITE)).unwrap();
        assert_eq!(ui.get("diff_deleted"), Some(Color::WHITE));
        assert_eq!(ui.get("nope"), None);
        let missing = UiColors::from_lookup(|k| (k != "hover").then_some(Color::BLACK));
        assert_eq!(missing, Err("hover"));
    }

    #[test]
    fn terminal_lookup_by_name_and_index() {
        let term = TerminalColors::from_lookup(|k| {
            Some(if k == "bright_red" {
                Color::WHITE
            } else {
                Color::BLACK
            })
        })
        .unwrap();
        assert_eq!(term.ansi[9], Color::WHITE);
        assert_eq!(term.get("bright_red"), Some(Color::WHITE));
        assert_eq!(term.ansi_color(16), None);
        assert_eq!(TerminalColors::FIELDS[18], "bright_white");
        assert_eq!(
            TerminalColors::from_lookup(|k| (k != "cursor").then_some(Color::BLACK)),
            Err("cursor")
        );
    }
}
