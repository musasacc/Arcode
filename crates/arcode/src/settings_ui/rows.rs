//! Rows of the Settings page bound to one setting: a switch, a stepper or
//! any control, with the setting's key and a reset button while its value
//! is not the default.

use std::rc::Rc;

use arcode_config::config::keys;
use arcode_config::{SettingEdit, SettingValue};
use arcode_ui_kit::{settings, switch};
use gpui::{App, SharedString, prelude::*};

use super::SettingsPage;
use super::widgets::{self, Reset, RowKey};

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
    /// A row for the setting at `path` with any control; a changed value
    /// gets a reset button that writes the default back.
    pub(crate) fn keyed_row(
        &self,
        label: impl Into<SharedString>,
        description: impl Into<SharedString>,
        path: &str,
        control: impl IntoElement,
        cx: &App,
    ) -> gpui::Div {
        let config = settings(cx);
        let low_end = config.ui.low_end_mode;
        let reset =
            (!keys::is_default(config, path)).then(|| vec![keys::reset_edit(path, low_end)]);
        self.keyed_row_with_reset(label, description, path, reset, control, cx)
    }

    /// A row for the setting at `path` whose reset writes `reset` (`None`:
    /// no reset button).
    pub(crate) fn keyed_row_with_reset(
        &self,
        label: impl Into<SharedString>,
        description: impl Into<SharedString>,
        path: &str,
        reset: Option<Vec<SettingEdit>>,
        control: impl IntoElement,
        cx: &App,
    ) -> gpui::Div {
        let low_end = settings(cx).ui.low_end_mode;
        let reset = reset.map(|edits| {
            let writer = self.writer();
            Reset {
                tooltip: format!("Reset to default ({})", keys::shown_default(path, low_end))
                    .into(),
                click: Rc::new(move |_, cx| writer.write(edits.clone(), cx)),
            }
        });
        let key = RowKey {
            path: SharedString::from(path.to_owned()),
            reset,
        };
        widgets::keyed_setting_row(label, description, key, control, cx)
    }

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
        self.keyed_row(label, description, path, control, cx)
    }

    /// A switch that low-end mode turns off: while it is on, the row says
    /// so in `low_end_note` and shows the switch off and disabled (nothing
    /// to change or reset then).
    pub(crate) fn low_end_bool_row(
        &self,
        label: &'static str,
        description: &'static str,
        low_end_note: &'static str,
        path: &'static str,
        on: bool,
        cx: &App,
    ) -> gpui::Div {
        if settings(cx).ui.low_end_mode {
            let control = switch(SharedString::from(path), false).disabled(true);
            self.keyed_row_with_reset(label, low_end_note, path, None, control, cx)
        } else {
            self.bool_row(label, description, path, on, cx)
        }
    }

    /// A switch that low-end mode turns off but leaves free to change:
    /// while low-end mode is on and the switch is off, the row says why in
    /// `low_end_note`.
    pub(crate) fn low_end_off_row(
        &self,
        label: &'static str,
        description: &'static str,
        low_end_note: &'static str,
        path: &'static str,
        on: bool,
        cx: &App,
    ) -> gpui::Div {
        let low_end = settings(cx).ui.low_end_mode;
        let description = if low_end && !on {
            low_end_note
        } else {
            description
        };
        self.bool_row(label, description, path, on, cx)
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
        self.keyed_row(label, description, path, control, cx)
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
