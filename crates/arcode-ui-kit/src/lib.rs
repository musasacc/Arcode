//! Shared GPUI layer for Arcode: theme and settings globals, embedded assets,
//! icons and a few small components. Views across crates use this so they
//! look consistent and read colors from one place.

mod assets;
mod color_key;
mod components;
mod font_fallback;
mod icon;
mod settings;
mod theme;

pub use assets::{Assets, BUNDLED_MONO_FONT, load_fonts};
pub use color_key::ColorKey;
pub use components::*;
pub use font_fallback::{apply_glyph_fallbacks, keep_font_runs_distinct};
pub use icon::{Icon, IconName, icon};
pub use settings::{Settings, animation_duration, set_settings, settings};
pub use theme::{ActiveTheme, TerminalColors, ThemeColors, UiTheme, hsla, set_theme, theme};
