//! Chooses the active theme from the settings and the OS appearance, and
//! switches it live when the appearance changes.

use std::path::Path;
use std::sync::Arc;

use arcode_config::{Theme, ThemeRegistry};
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

/// The theme editor's edited theme: while the editor is open it stays
/// shown, also when the settings or the OS appearance change.
#[derive(Default)]
struct EditedTheme(Option<Arc<Theme>>);

impl Global for EditedTheme {}

/// The edited theme shown while the theme editor is open.
fn edited(cx: &App) -> Option<Arc<Theme>> {
    cx.try_global::<EditedTheme>().and_then(|e| e.0.clone())
}

/// Resolves `ui.theme` for the given OS appearance and makes it active if
/// it differs from the current theme. While the theme editor is open, its
/// edited theme is shown instead.
pub fn apply(appearance: WindowAppearance, cx: &mut App) {
    if let Some(edited) = edited(cx) {
        let shown = cx
            .try_global::<arcode_ui_kit::ActiveTheme>()
            .is_some_and(|_| Arc::ptr_eq(&theme(cx).source, &edited));
        if !shown {
            set_theme(edited, cx);
        }
        return;
    }
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
pub fn preview(theme: &Theme, cx: &mut App) {
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
/// replaces the registry; the task ends when the new registry is in place,
/// with the loader's warnings (also logged).
pub fn reload_themes(cx: &mut App) -> Task<Vec<String>> {
    let dir = arcode_config::paths::themes_dir();
    let load = cx.background_spawn(async move { ThemeRegistry::load(dir.as_deref()) });
    cx.spawn(async move |cx| {
        let (registry, warnings) = load.await;
        for warning in &warnings {
            log::warn!("theme: {warning}");
        }
        cx.update(|cx| cx.set_global(Themes(registry))).ok();
        warnings
    })
}

/// Shows `theme` even when a theme of the same name is shown (the theme
/// editor's live preview of an edited theme keeps its name), and keeps it
/// shown until [`restore`] or [`end_edit`].
pub fn show_edited(theme: &Theme, cx: &mut App) {
    let theme = Arc::new(theme.clone());
    cx.set_global(EditedTheme(Some(theme.clone())));
    set_theme(theme, cx);
}

/// The theme editor closed: [`apply`] shows the configured theme again
/// from now on. The theme shown does not change now (a theme just saved
/// and selected stays).
pub fn end_edit(cx: &mut App) {
    if edited(cx).is_some() {
        cx.set_global(EditedTheme(None));
    }
}

/// Shows the configured theme again, also when the theme shown has its
/// name (an edited copy of it), and ends the theme editor's preview.
/// Unlike [`apply`], it always re-themes.
pub fn restore(appearance: WindowAppearance, cx: &mut App) {
    end_edit(cx);
    let ui = &settings(cx).ui;
    let resolved = cx.global::<Themes>().0.resolve(
        &ui.theme,
        &ui.dark_theme,
        &ui.light_theme,
        is_dark(appearance),
    );
    let resolved = Arc::new(resolved.clone());
    set_theme(resolved, cx);
}

/// Reads the themes folder again and shows the configured theme from it,
/// so a theme file saved in an editor tab takes effect at once. Ends with
/// the loader's warnings.
pub fn reload_and_restore(cx: &mut App) -> Task<Vec<String>> {
    let reload = reload_themes(cx);
    cx.spawn(async move |cx| {
        let warnings = reload.await;
        cx.update(|cx| restore(cx.window_appearance(), cx)).ok();
        warnings
    })
}

/// The message to show for the loader's warnings about `file` (a theme
/// file just saved), or `None` when it loaded cleanly. The loader prefixes
/// each warning with the file's path.
pub fn file_warning(warnings: &[String], file: &Path) -> Option<String> {
    let name = file.file_name()?;
    let (_, problem) = warnings.iter().find_map(|w| {
        let (shown, problem) = w.split_once(": ")?;
        (Path::new(shown).file_name() == Some(name)).then_some((shown, problem))
    })?;
    let shown = name.to_string_lossy();
    let (problem, skipped) = match problem.strip_suffix("; skipped") {
        Some(problem) => (problem, true),
        None => (problem, false),
    };
    let first = problem.lines().next().unwrap_or_default().trim();
    Some(if skipped {
        format!("{shown} was not loaded: {first}")
    } else {
        format!("{shown}: {first}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_warnings_name_the_saved_file() {
        let warnings = vec![
            "/cfg/themes/other.toml: unknown key `x`".to_owned(),
            "/cfg/themes/my-ember.toml: not valid TOML: TOML parse error at line 9, column 6\n  |\n9 | oops = = 1\n; skipped".to_owned(),
        ];
        assert_eq!(
            file_warning(&warnings, Path::new("/cfg/themes/my-ember.toml")).as_deref(),
            Some(
                "my-ember.toml was not loaded: not valid TOML: TOML parse error at line 9, column 6"
            )
        );
        assert_eq!(
            file_warning(&warnings, Path::new("/x/other.toml")).as_deref(),
            Some("other.toml: unknown key `x`")
        );
        assert_eq!(
            file_warning(&warnings, Path::new("/cfg/themes/mine.toml")),
            None
        );
        assert_eq!(
            file_warning(&[], Path::new("/cfg/themes/my-ember.toml")),
            None
        );
    }
}
