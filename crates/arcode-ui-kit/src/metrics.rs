//! Fixed sizes of the Ember/Chalk workbench (docs/DESIGN.md 5.1) and the
//! type scale of section 4.
//!
//! Heights never depend on the font: if Inter fails to load and the system
//! UI font is used instead, rows, chips and the footer still align.

use gpui::{App, FontWeight, Pixels, px};

use crate::Settings;

/// Canvas between panels and at the right window edge; resize handles
/// live in these gaps.
pub const GAP: Pixels = px(6.);
/// Title bar height (drawn on the canvas).
pub const TITLE_BAR_H: Pixels = px(40.);
/// Icon rail width; the rail column is the left gutter.
pub const RAIL_W: Pixels = px(46.);
/// Footer height (drawn on the canvas, starts at the rail's right edge).
pub const FOOTER_H: Pixels = px(26.);
/// Corner radius of every floating panel.
pub const PANEL_RADIUS: Pixels = px(10.);
/// Corner radius of the palette and dialogs.
pub const OVERLAY_RADIUS: Pixels = px(12.);
/// Corner radius of popovers and menus.
pub const POPOVER_RADIUS: Pixels = px(10.);
/// Height of title-bar chips, buttons, inputs and search fields.
pub const CONTROL_H: Pixels = px(28.);
/// Control height inside panels.
pub const CONTROL_H_PANEL: Pixels = px(26.);
/// Corner radius of buttons, inputs, tab pills and segmented controls.
pub const CONTROL_RADIUS: Pixels = px(8.);
/// Corner radius of icon buttons.
pub const ICON_BUTTON_RADIUS: Pixels = px(7.);
/// Tree and list row height.
pub const ROW_H: Pixels = px(24.);
/// Row height in the debugger columns.
pub const ROW_H_DENSE: Pixels = px(22.);
/// Row height in the command palette and pickers.
pub const ROW_H_PALETTE: Pixels = px(32.);
/// Row height of recent projects on the welcome screen.
pub const ROW_H_WELCOME: Pixels = px(40.);
/// Rows are chips inset this far from the panel edges.
pub const ROW_INSET: Pixels = px(6.);
/// Corner radius of row chips.
pub const ROW_RADIUS: Pixels = px(6.);
/// Height of the editor tab strip and the bottom panel header.
pub const TAB_STRIP_H: Pixels = px(38.);
/// Height of a tab pill.
pub const TAB_H: Pixels = px(26.);
/// Height of a terminal-session pill.
pub const TAB_H_SMALL: Pixels = px(24.);
/// Size of a rail tile.
pub const RAIL_TILE: Pixels = px(34.);
/// Corner radius of a rail tile.
pub const RAIL_TILE_RADIUS: Pixels = px(9.);
/// Vertical distance between rail tiles.
pub const RAIL_PITCH: Pixels = px(38.);
/// Default sidebar width.
pub const SIDEBAR_W: Pixels = px(240.);
/// Default bottom panel height.
pub const BOTTOM_H: Pixels = px(228.);
/// Size of the ember mark (diamond).
pub const EMBER: Pixels = px(7.);
/// Height of a panel title row (sidebar views).
pub const PANEL_TITLE_H: Pixels = px(40.);
/// Height of a section header inside a panel.
pub const SECTION_H: Pixels = px(30.);

/// Icon sizes of section 5.1.
pub mod icon_size {
    use gpui::{Pixels, px};

    /// Rail icons.
    pub const RAIL: Pixels = px(18.);
    /// Tabs, tree rows, palette rows.
    pub const ROW: Pixels = px(15.);
    /// Toolbars and headers.
    pub const TOOLBAR: Pixels = px(14.);
    /// Footer and inline icons.
    pub const INLINE: Pixels = px(13.);
}

/// Inter Regular: body text.
pub const WEIGHT_REGULAR: FontWeight = FontWeight::NORMAL;
/// Inter Medium: active tab, selected row, buttons, footer.
pub const WEIGHT_MEDIUM: FontWeight = FontWeight::MEDIUM;
/// Inter SemiBold: titles and section headers.
pub const WEIGHT_SEMIBOLD: FontWeight = FontWeight::SEMIBOLD;

/// Font sizes of the type scale, in whole pixels, derived from
/// `ui.font_size` (13 by default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    /// `font_size - 2` (11): kbd chips, count badges.
    pub micro: Pixels,
    /// `font_size - 1` (12): footer, breadcrumbs, descriptions, section
    /// headers.
    pub meta: Pixels,
    /// `font_size - 1` (12), set in mono: Ln/Col, counts, durations.
    pub readout: Pixels,
    /// `font_size` (13): rows, tabs, lists, menus, buttons.
    pub body: Pixels,
    /// `font_size + 1` (14): panel titles, settings group headings.
    pub title: Pixels,
    /// `font_size + 2` (15): the palette query.
    pub input: Pixels,
    /// `font_size + 7` (20): the settings page title.
    pub heading: Pixels,
    /// `font_size + 15` (28): the welcome title.
    pub display: Pixels,
}

impl TypeScale {
    /// The scale for a UI font size (rounded to whole pixels, at least 8,
    /// so half sizes never rasterise soft and nothing reaches zero).
    pub fn new(font_size: f32) -> Self {
        let base = if font_size.is_finite() {
            font_size.round().max(8.)
        } else {
            13.
        };
        let at = |delta: f32| px(base + delta);
        TypeScale {
            micro: at(-2.),
            meta: at(-1.),
            readout: at(-1.),
            body: at(0.),
            title: at(1.),
            input: at(2.),
            heading: at(7.),
            display: at(15.),
        }
    }
}

impl Default for TypeScale {
    fn default() -> Self {
        Self::new(13.)
    }
}

/// The type scale for the current `ui.font_size` (the default scale when
/// no settings are installed, as in some tests).
pub fn type_scale(cx: &App) -> TypeScale {
    cx.try_global::<Settings>()
        .map_or_else(TypeScale::default, |s| TypeScale::new(s.0.ui.font_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_follows_the_ui_font_size() {
        let s = TypeScale::default();
        assert_eq!(
            [s.micro, s.meta, s.readout, s.body, s.title, s.input],
            [px(11.), px(12.), px(12.), px(13.), px(14.), px(15.)]
        );
        assert_eq!((s.heading, s.display), (px(20.), px(28.)));
        let big = TypeScale::new(15.4);
        assert_eq!(
            (big.body, big.micro, big.display),
            (px(15.), px(13.), px(30.))
        );
        assert_eq!(TypeScale::new(2.).micro, px(6.));
        assert_eq!(TypeScale::new(f32::NAN), TypeScale::default());
    }

    #[test]
    fn metrics_match_the_design() {
        assert_eq!(GAP, px(6.));
        assert_eq!((TITLE_BAR_H, FOOTER_H, RAIL_W), (px(40.), px(26.), px(46.)));
        assert_eq!(TAB_STRIP_H - TAB_H, px(12.), "pills sit centred");
        assert!(RAIL_PITCH > RAIL_TILE);
    }
}
