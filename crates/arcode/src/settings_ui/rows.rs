//! Rows of the Settings page bound to one setting: a switch or a stepper.

use arcode_config::SettingValue;
use gpui::{App, SharedString};

use super::{SettingsPage, widgets};

/// The range and step of a numeric setting.
#[derive(Clone, Copy)]
pub(crate) struct Steps {
    pub step: f32,
    pub min: f32,
    pub max: f32,
    /// Written as an integer.
    pub int: bool,
}

impl SettingsPage {
    /// A row with a switch writing `path`.
    pub(crate) fn bool_row(
        &self,
        label: &'static str,
        description: &'static str,
        path: &'static str,
        on: bool,
        cx: &App,
    ) -> gpui::Div {
        let writer = self.writer();
        let control = widgets::toggle(SharedString::from(path), on, move |value, _, cx| {
            writer.set(path, value, cx)
        });
        widgets::setting_row(label, description, control, cx)
    }

    /// A row with a stepper writing `path`.
    pub(crate) fn number_row(
        &self,
        label: &'static str,
        description: &'static str,
        path: &'static str,
        value: f32,
        steps: Steps,
        cx: &App,
    ) -> gpui::Div {
        let writer = self.writer();
        let shown = shown_value(value, steps);
        let control = widgets::stepper(
            path,
            shown,
            move |delta, _, cx| {
                // Step from a value still being written (quick clicks).
                let base = match crate::prefs::store::pending_value(path, cx) {
                    Some(SettingValue::Float(f)) => f as f32,
                    Some(SettingValue::Int(i)) => i as f32,
                    _ => value,
                };
                let next = widgets::stepped(base, delta, steps.step, steps.min, steps.max);
                if next != base {
                    if steps.int {
                        writer.set(path, next.round() as u64, cx);
                    } else {
                        writer.set(path, next, cx);
                    }
                }
            },
            cx,
        );
        widgets::setting_row(label, description, control, cx)
    }
}

/// A stepper's text: whole numbers ("13") for integer settings and for
/// whole values of whole steps (the UI size), else one decimal ("1.5").
pub(super) fn shown_value(value: f32, steps: Steps) -> String {
    let whole = steps.step.fract() == 0. && value.fract() == 0.;
    if steps.int || whole {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

/// The keymap choice shown (0 Default, 1 Vim): Vim also when `vim_mode`
/// is on.
pub(crate) fn keymap_index(config: &arcode_config::Config) -> usize {
    let e = &config.editor;
    usize::from(e.vim_mode || e.keymap == arcode_config::Keymap::Vim)
}
