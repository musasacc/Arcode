//! Chooses the active theme from the settings and the OS appearance, and
//! switches it live when the appearance changes.

use std::sync::Arc;

use arcode_config::ThemeRegistry;
use arcode_ui_kit::{set_theme, settings, theme};
use gpui::{App, AppContext, Global, Task, WindowAppearance};

/// Global holding every available theme.
pub struct Themes(pub ThemeRegistry);

impl Global for Themes {}

/// Whether an OS appearance is dark.
pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Loads the built-in and user themes into the [`Themes`] global.
pub fn init(cx: &mut App) {
    let dir = arcode_config::paths::themes_dir();
    let (registry, warnings) = ThemeRegistry::load(dir.as_deref());
    for warning in warnings {
        log::warn!("theme: {warning}");
    }
    cx.set_global(Themes(registry));
}

/// Resolves `ui.theme` for the given OS appearance and makes it active if
/// it differs from the current theme.
pub fn apply(appearance: WindowAppearance, cx: &mut App) {
    let ui = &settings(cx).ui;
    let resolved = cx.global::<Themes>().0.resolve(
        &ui.theme,
        &ui.dark_theme,
        &ui.light_theme,
        is_dark(appearance),
    );
    let unchanged = cx
        .try_global::<arcode_ui_kit::ActiveTheme>()
        .is_some_and(|_| theme(cx).name == resolved.name);
    if !unchanged {
        log::info!("theme: {}", resolved.name);
        let resolved = Arc::new(resolved.clone());
        set_theme(resolved, cx);
    }
}

/// Shows `theme` without changing the settings (a picker's live preview).
/// [`apply`] goes back to the configured theme.
pub fn preview(theme: &arcode_config::Theme, cx: &mut App) {
    if theme.name != active_name(cx) {
        set_theme(Arc::new(theme.clone()), cx);
    }
}

/// The name of the theme shown now.
pub fn active_name(cx: &App) -> String {
    cx.try_global::<arcode_ui_kit::ActiveTheme>()
        .map(|_| theme(cx).name.clone())
        .unwrap_or_default()
}

/// Reads the user themes folder again on the background executor and
/// replaces the registry; the task ends when the new registry is in place.
pub fn reload_themes(cx: &mut App) -> Task<()> {
    let dir = arcode_config::paths::themes_dir();
    let load = cx.background_spawn(async move { ThemeRegistry::load(dir.as_deref()) });
    cx.spawn(async move |cx| {
        let (registry, warnings) = load.await;
        for warning in warnings {
            log::warn!("theme: {warning}");
        }
        cx.update(|cx| cx.set_global(Themes(registry))).ok();
    })
}
