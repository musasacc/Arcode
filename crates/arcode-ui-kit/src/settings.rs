//! The loaded user configuration as a GPUI global.

use std::{sync::Arc, time::Duration};

use arcode_config::Config;
use gpui::{App, Global};

/// Global holding the active [`Config`].
pub struct Settings(pub Arc<Config>);

impl Global for Settings {}

/// The active settings. Panics if [`set_settings`] was never called.
pub fn settings(cx: &App) -> &Config {
    &cx.global::<Settings>().0
}

/// Replaces the active settings and refreshes every window.
pub fn set_settings(config: Config, cx: &mut App) {
    cx.set_global(Settings(Arc::new(config)));
    cx.refresh_windows();
}

/// Duration for UI transitions, or `None` when animations are off
/// (disabled in settings or low-end mode). Callers skip the animation
/// entirely on `None` so no extra frames are scheduled.
pub fn animation_duration(cx: &App) -> Option<Duration> {
    let ui = &settings(cx).ui;
    (ui.animations && !ui.low_end_mode).then(|| Duration::from_millis(ui.animation_ms as u64))
}
