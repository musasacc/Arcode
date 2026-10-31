//! The first-run dialog, shown when no `config.toml` exists: theme (with a
//! live preview), editor font, keymap and performance mode. It writes
//! `config.toml` (the commented template with the choices filled in), then
//! offers the tour. Escape (Skip) writes the template unchanged; when the
//! file cannot be written, Skip still closes the dialog (it is modal) and
//! the failure is reported.

use arcode_config::{FirstRunChoices, ThemeChoice};
use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{
    BUNDLED_MONO_FONT, ButtonStyle, button, font_choices, logo, looks_monospace, overlay, theme,
};
use gpui::{
    App, AppContext, Context, EventEmitter, FocusHandle, Focusable, Global, KeyBinding,
    SharedString, Task, Window, actions, div, prelude::*, px,
};

use super::store::write_settings;
use crate::theme_sync::{self, Themes};

actions!(
    first_run,
    [
        /// Saves the choices (or starts the tour once saved).
        Accept,
        /// Skips the dialog (or the tour once saved).
        Skip,
    ]
);

/// Key context of the dialog.
pub const CONTEXT: &str = "FirstRun";

/// Padding around the dialog's content.
const PADDING: f32 = 20.;

/// Enter accepts, Escape skips.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Accept, Some(CONTEXT)),
        KeyBinding::new("escape", Skip, Some(CONTEXT)),
    ]);
}

/// Set at startup when there is no configuration file; the first window
/// takes it and shows the dialog.
pub struct FirstRunPending;

impl Global for FirstRunPending {}

/// Whether the dialog should be shown at startup: no `config.toml` yet
/// and `ARCODE_SKIP_FIRST_RUN` not set (automation, CI).
pub fn wanted(config_exists: bool, skip_env: Option<&str>) -> bool {
    !config_exists && skip_env.is_none_or(|v| v.is_empty() || v == "0")
}

/// Theme cards offered: follow the system, Arcode Ember and Arcode Chalk,
/// then the Classic themes (docs/DESIGN.md 6.16).
pub(super) const THEME_CARDS: [(&str, &str); 6] = [
    ("system", "Follow system"),
    ("Arcode Ember", "Arcode Ember"),
    ("Arcode Chalk", "Arcode Chalk"),
    ("Arcode Dark", "Arcode Dark"),
    ("Arcode Light", "Arcode Light"),
    ("Arcode Midnight", "Arcode Midnight"),
];

/// What the dialog reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstRunEvent {
    /// The dialog is done; start the tour when `tour`.
    Finished {
        /// Whether to start the tour.
        tour: bool,
    },
    /// Writing the configuration failed.
    Failed(SharedString),
}

/// The dialog.
pub struct FirstRunDialog {
    focus: FocusHandle,
    /// The choices so far.
    pub choices: FirstRunChoices,
    /// Monospace fonts offered (the bundled one first).
    pub(super) fonts: Vec<String>,
    /// The file is written; the tour is offered.
    pub saved: bool,
    /// Why the last save failed.
    error: Option<SharedString>,
    writing: Option<Task<()>>,
    _fonts_task: Task<()>,
}

impl EventEmitter<FirstRunEvent> for FirstRunDialog {}

impl Focusable for FirstRunDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FirstRunDialog {
    /// A dialog showing the current settings as its choices (the defaults
    /// on a first run); system fonts load in the background.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let choices = FirstRunChoices::from_config(arcode_ui_kit::settings(cx));
        let text_system = cx.text_system().clone();
        let names = cx.background_spawn(async move { text_system.all_font_names() });
        let fonts_task = cx.spawn(async move |this, cx| {
            let names = names.await;
            this.update(cx, |dialog, cx| {
                let current = dialog.choices.editor_font.clone();
                let mut mono: Vec<String> = font_choices(names)
                    .into_iter()
                    .filter(|f| f == BUNDLED_MONO_FONT || looks_monospace(f))
                    .take(4)
                    .collect();
                if !mono.contains(&current) {
                    mono.insert(0, current);
                }
                if mono != dialog.fonts {
                    dialog.fonts = mono;
                    cx.notify();
                }
            })
            .ok();
        });
        Self {
            focus: cx.focus_handle(),
            fonts: initial_fonts(&choices.editor_font),
            choices,
            saved: false,
            error: None,
            writing: None,
            _fonts_task: fonts_task,
        }
    }

    /// Picks a theme and previews it.
    pub fn choose_theme(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.choices.theme = ThemeChoice::parse(value);
        let config = arcode_ui_kit::settings(cx);
        let themes = &cx.global::<Themes>().0;
        let shown = themes
            .resolve(
                &self.choices.theme,
                &config.ui.dark_theme,
                &config.ui.light_theme,
                theme_sync::is_dark(window.appearance()),
            )
            .clone();
        theme_sync::preview(&shown, cx);
        cx.notify();
    }

    /// Writes the file: the choices (`accept`) or the plain template.
    pub fn save(&mut self, accept: bool, cx: &mut Context<Self>) {
        if self.writing.is_some() || self.saved {
            return;
        }
        let edits = if accept {
            self.choices.edits()
        } else {
            Vec::new()
        };
        self.error = None;
        let task = write_settings(edits, cx);
        self.writing = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |dialog, cx| {
                dialog.writing = None;
                match result {
                    Ok(()) if accept => dialog.saved = true,
                    Ok(()) => cx.emit(FirstRunEvent::Finished { tour: false }),
                    // Skipping always leaves the modal dialog.
                    Err(err) if !accept => {
                        cx.emit(FirstRunEvent::Failed(err.into()));
                        cx.emit(FirstRunEvent::Finished { tour: false });
                    }
                    Err(err) => {
                        let err = SharedString::from(err);
                        dialog.error = Some(err.clone());
                        cx.emit(FirstRunEvent::Failed(err));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn accept(&mut self, _: &Accept, _: &mut Window, cx: &mut Context<Self>) {
        if self.saved {
            cx.emit(FirstRunEvent::Finished { tour: true });
        } else {
            self.save(true, cx);
        }
    }

    fn skip(&mut self, _: &Skip, _: &mut Window, cx: &mut Context<Self>) {
        if self.saved {
            cx.emit(FirstRunEvent::Finished { tour: false });
        } else {
            self.save(false, cx);
        }
    }
}

impl Render for FirstRunDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let max_h = window.viewport_size().height - px(24.);
        let c = theme(cx).colors.clone();
        let (title, subtitle) = if self.saved {
            (
                "You're all set",
                "Settings are saved in config.toml. Take a one-minute tour of the workspace?",
            )
        } else {
            (
                "Welcome to Arcode",
                "A few choices to get started. Everything can be changed later in Settings.",
            )
        };
        let (skip, accept) = if self.saved {
            ("Not Now", "Start Tour")
        } else {
            ("Skip", "Save and Continue")
        };
        let busy = self.writing.is_some();
        let scale = type_scale(cx);
        overlay(cx)
            .id("first-run")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::skip))
            .w(super::first_run_choices::content_width() + px(2. * PADDING))
            .max_h(max_h)
            .overflow_y_scroll()
            .gap_3()
            .p(px(PADDING))
            .text_color(c.text)
            .text_size(scale.body)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(logo(px(22.)))
                    .child(
                        div()
                            .text_size(scale.title)
                            .font_weight(WEIGHT_SEMIBOLD)
                            .child(title),
                    ),
            )
            .child(div().text_color(c.text_muted).child(subtitle))
            .when(!self.saved, |el| el.child(self.render_choices(cx)))
            .when_some(self.error.clone(), |el, error| {
                el.child(
                    div()
                        .text_size(scale.meta)
                        .text_color(c.error)
                        .child(format!("{error}. Skip closes this dialog without saving.")),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("first-run-skip", skip)
                            .style(ButtonStyle::Ghost)
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Skip), cx)),
                    )
                    .child(
                        button("first-run-accept", if busy { "Saving…" } else { accept })
                            .style(ButtonStyle::Primary)
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Accept), cx)),
                    ),
            )
    }
}

/// The fonts offered before the system list loads: the current one and
/// the bundled one.
fn initial_fonts(current: &str) -> Vec<String> {
    let mut fonts = vec![current.to_owned()];
    if current != BUNDLED_MONO_FONT {
        fonts.push(BUNDLED_MONO_FONT.to_owned());
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ember_and_chalk_come_first_then_every_classic_theme() {
        let values: Vec<&str> = THEME_CARDS.iter().map(|(v, _)| *v).collect();
        assert_eq!(
            values[..3],
            [
                "system",
                arcode_config::DEFAULT_DARK_THEME,
                arcode_config::DEFAULT_LIGHT_THEME
            ]
        );
        for name in arcode_config::theme::BUILTIN_THEME_NAMES {
            assert!(values.contains(&name), "{name} is offered");
        }
    }

    #[test]
    fn shown_only_without_a_config_and_without_the_opt_out() {
        assert!(wanted(false, None));
        assert!(wanted(false, Some("0")));
        assert!(!wanted(true, None));
        assert!(!wanted(false, Some("1")));
    }
}
