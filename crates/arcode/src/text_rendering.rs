//! `ui.text_gamma` and `ui.text_contrast` at startup.
//!
//! GPUI's Linux renderer reads its text gamma and contrast from the
//! environment when it creates a window's renderer, so the settings are
//! handed over as environment variables before the application starts, and
//! a change takes effect at the next start. What Arcode started with is
//! kept so the Settings page can say when a restart is needed, and which
//! values come from the environment rather than the settings.

use arcode_config::UiSettings;
use gpui::{App, Global};

/// The text gamma and contrast the renderer was started with.
#[derive(Clone, Debug, PartialEq)]
pub struct StartedWith {
    /// `ui.text_gamma` at startup.
    pub gamma: f32,
    /// `ui.text_contrast` at startup.
    pub contrast: f32,
    /// Variables set outside Arcode that the renderer uses because the
    /// matching setting was left at its default.
    pub outside: Vec<(&'static str, String)>,
}

impl Global for StartedWith {}

impl StartedWith {
    /// The values of `ui`, with nothing from outside Arcode.
    pub fn of(ui: &UiSettings) -> Self {
        Self {
            gamma: ui.text_gamma,
            contrast: ui.text_contrast,
            outside: Vec::new(),
        }
    }

    /// Whether `ui` differs from these values, so a restart is needed to
    /// see it.
    pub fn differs_from(&self, ui: &UiSettings) -> bool {
        self.gamma != ui.text_gamma || self.contrast != ui.text_contrast
    }
}

/// Sets and removes the environment variables GPUI's renderer reads, as
/// [`UiSettings::text_rendering_env`] says (nothing on macOS and Windows,
/// where the settings do nothing), and returns what the renderer starts
/// with.
///
/// Must run before any other thread exists: `std::env::set_var` races with
/// any thread reading the environment.
pub(crate) fn export_env(ui: &UiSettings) -> StartedWith {
    let plan = ui.text_rendering_env(|name| std::env::var(name).ok());
    for (name, value) in plan.changes {
        match value {
            Some(value) => {
                log::info!("text rendering: {name}={value}");
                // SAFETY: called from `app::run` right after the
                // configuration is read, before GPUI, the logger's writers,
                // the language server signal handler or anything else has
                // started a thread, so nothing can read the environment
                // concurrently.
                unsafe { std::env::set_var(name, value) };
            }
            None => {
                log::info!("text rendering: {name} removed");
                // SAFETY: as above.
                unsafe { std::env::remove_var(name) };
            }
        }
    }
    for (name, value) in &plan.outside {
        log::info!("text rendering: {name}={value} from the environment");
    }
    StartedWith {
        outside: plan.outside,
        ..StartedWith::of(ui)
    }
}

/// Whether the saved settings differ from the ones the renderer started
/// with, so a restart is needed to see them. False when the start values
/// are unknown (tests) or the settings do nothing on this platform.
pub fn restart_needed(ui: &UiSettings, cx: &App) -> bool {
    arcode_config::TEXT_RENDERING_TUNABLE
        && cx
            .try_global::<StartedWith>()
            .is_some_and(|started| started.differs_from(ui))
}

/// The variables from outside Arcode the renderer uses, for the Settings
/// page; empty when there are none or the start values are unknown.
pub fn outside_values(cx: &App) -> &[(&'static str, String)] {
    cx.try_global::<StartedWith>()
        .map_or(&[], |started| started.outside.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui(gamma: f32, contrast: f32) -> UiSettings {
        UiSettings {
            text_gamma: gamma,
            text_contrast: contrast,
            ..UiSettings::default()
        }
    }

    #[test]
    fn started_with_compares_the_settings() {
        let started = StartedWith::of(&ui(2.0, 3.0));
        assert!(!started.differs_from(&ui(2.0, 3.0)));
        assert!(started.differs_from(&ui(2.1, 3.0)));
        assert!(started.differs_from(&ui(2.0, 1.0)));
        // A value from the environment is not a setting.
        let started = StartedWith {
            outside: vec![(arcode_config::TEXT_GAMMA_ENV, "2.2".into())],
            ..StartedWith::of(&UiSettings::default())
        };
        assert!(!started.differs_from(&UiSettings::default()));
    }

    #[gpui::test]
    fn restart_is_needed_once_the_settings_change(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            // Unknown start values (tests, previews): never.
            assert!(!restart_needed(&ui(2.2, 5.0), cx));
            assert!(outside_values(cx).is_empty());
            cx.set_global(StartedWith {
                outside: vec![(arcode_config::TEXT_CONTRAST_ENV, "3".into())],
                ..StartedWith::of(&ui(2.0, 1.0))
            });
            assert!(!restart_needed(&ui(2.0, 1.0), cx));
            assert_eq!(
                restart_needed(&ui(2.2, 1.0), cx),
                arcode_config::TEXT_RENDERING_TUNABLE
            );
            assert_eq!(
                outside_values(cx),
                [(arcode_config::TEXT_CONTRAST_ENV, "3".to_string())]
            );
        });
    }
}
