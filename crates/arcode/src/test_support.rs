//! Shared setup for GPUI tests of the app's views.

use std::sync::Arc;
use std::time::Duration;

use arcode_config::{Config, ThemeRegistry};
use gpui::TestAppContext;

/// Installs settings, a theme, in-memory app state, the editor and the
/// app's key bindings.
pub fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        cx.set_global(crate::theme_sync::Themes(registry));
        crate::app_state::init_in_memory(cx);
        arcode_editor::init(cx);
        crate::actions::bind_keys(cx);
        crate::text_field::bind_keys(cx);
        crate::finder::bind_keys(cx);
        crate::find_bar::bind_keys(cx);
        crate::search_view::bind_keys(cx);
        crate::commands::init(cx);
    });
}

/// Lets timers (debounces, batching) fire and background work finish.
pub fn settle(cx: &mut TestAppContext) {
    for _ in 0..8 {
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
    }
}
