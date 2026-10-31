//! Shared GPUI layer for Arcode: theme and settings globals, embedded assets,
//! icons, metrics and the components of the Ember/Chalk look
//! (docs/DESIGN.md). Views across crates use this so they look consistent
//! and read colors and sizes from one place.

mod assets;
mod color_key;
mod components;
mod effects;
mod font_fallback;
mod icon;
pub mod metrics;
mod settings;
mod theme;

pub use assets::{
    Assets, BUNDLED_MONO_FONT, BUNDLED_UI_FONT, SYSTEM_UI_FONT, load_fonts, mono_font_family,
    ui_font_family,
};
pub use color_key::ColorKey;
pub use components::*;
pub use effects::{Elevated, debounce, font_choices, looks_monospace, low_end};
pub use font_fallback::{apply_glyph_fallbacks, glyph_advance, keep_font_runs_distinct};
pub use icon::{Icon, IconName, icon};
pub use metrics::{TypeScale, type_scale};
pub use settings::{Settings, animation_duration, set_settings, settings};
pub use theme::{
    ActiveTheme, TerminalColors, ThemeColors, UiTheme, accent_hover, hsla, set_theme, shadow_color,
    theme,
};
