//! The New Theme dialog (docs/DESIGN.md 6.12): a name and the theme to
//! start from (every built-in and user theme, the one in use first
//! selected). Enter creates, Escape cancels, Up and Down pick the theme.

use arcode_config::{Appearance, ThemeRegistry};
use arcode_ui_kit::{
    ButtonStyle, IconName, button, icon, input_frame, list_row, theme, type_scale,
};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ScrollHandle, SharedString, Window, actions, div, prelude::*, px,
};

use crate::picker_frame::{dialog_buttons, dialog_frame, dialog_title};
use crate::text_field::{TextChanged, TextField};

actions!(new_theme, [Create, Cancel, SelectPrevious, SelectNext]);

const CONTEXT: &str = "NewThemeDialog";

/// Height of the theme list before it scrolls.
const LIST_H: f32 = 232.;

/// Registers the dialog's keys.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Create, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
    ]);
}

/// What the dialog decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewThemeEvent {
    /// Create a theme called `name` (trimmed, not empty) from `base`.
    Create { name: String, base: String },
    /// Cancelled.
    Dismissed,
}

/// One theme the new one can start from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseTheme {
    pub name: SharedString,
    pub appearance: Appearance,
    pub builtin: bool,
}

/// The themes of `registry` in its order (built-ins first).
pub fn base_themes(registry: &ThemeRegistry) -> Vec<BaseTheme> {
    registry
        .themes()
        .iter()
        .map(|t| BaseTheme {
            name: t.name.clone().into(),
            appearance: t.appearance,
            builtin: arcode_config::theme::is_builtin_name(&t.name),
        })
        .collect()
}

/// A free name for a theme started from `base`: "My Ember", then "My
/// Ember 2", ... (a user theme's copy: "<name> copy").
pub fn suggested_name(base: &str, registry: &ThemeRegistry) -> String {
    let stem = if arcode_config::theme::is_builtin_name(base) {
        format!("My {}", base.trim_start_matches("Arcode "))
    } else {
        format!("{base} copy")
    };
    let mut name = stem.clone();
    let mut n = 2;
    while registry.get(&name).is_some() {
        name = format!("{stem} {n}");
        n += 1;
    }
    name
}

/// The dialog view.
pub struct NewThemeDialog {
    input: Entity<TextField>,
    themes: Vec<BaseTheme>,
    selected: usize,
    /// The theme in use (marked in the list).
    in_use: SharedString,
    hint: SharedString,
    error: bool,
    scroll: ScrollHandle,
}

impl EventEmitter<NewThemeEvent> for NewThemeDialog {}

impl Focusable for NewThemeDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

/// The hint under the list while nothing is wrong.
const HINT: &str = "Saved in your themes folder.";

impl NewThemeDialog {
    /// A dialog listing `themes`, starting from `in_use` (the theme in use)
    /// with a suggested name selected, so typing replaces it.
    pub fn new(themes: Vec<BaseTheme>, in_use: &str, name: &str, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut field = TextField::new("Theme name", cx);
            field.set_text(name, cx);
            field.select_all_text(cx);
            field
        });
        cx.subscribe(&input, |dialog, _, _: &TextChanged, cx| {
            if dialog.error {
                dialog.error = false;
                dialog.hint = HINT.into();
                cx.notify();
            }
        })
        .detach();
        let selected = themes.iter().position(|t| t.name == in_use).unwrap_or(0);
        Self {
            input,
            themes,
            selected,
            in_use: in_use.to_owned().into(),
            hint: HINT.into(),
            error: false,
            scroll: ScrollHandle::new(),
        }
    }

    /// The theme the new one starts from.
    pub fn base(&self) -> Option<&BaseTheme> {
        self.themes.get(self.selected)
    }

    /// The name typed.
    pub fn name(&self, cx: &App) -> String {
        self.input.read(cx).text().trim().to_owned()
    }

    /// Types `name` (tests).
    pub fn set_name(&mut self, name: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |f, cx| f.set_text(name, cx));
    }

    /// Picks the theme to start from by name.
    pub fn select_base(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.themes.iter().position(|t| t.name == name) {
            self.select(ix, cx);
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = ix.min(self.themes.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    /// Says why the name was refused (shown in the error colour).
    pub fn set_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.hint = message.into();
        self.error = true;
        cx.notify();
    }

    /// The hint shown, and whether it is an error (tests).
    pub fn hint(&self) -> (&str, bool) {
        (&self.hint, self.error)
    }

    /// Asks to create the theme (Enter or the Create button).
    pub fn create(&mut self, _: &Create, _: &mut Window, cx: &mut Context<Self>) {
        let name = self.name(cx);
        if name.is_empty() {
            self.set_error("Enter a name for the theme.", cx);
            return;
        }
        let Some(base) = self.base() else { return };
        let base = base.name.to_string();
        cx.emit(NewThemeEvent::Create { name, base });
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(NewThemeEvent::Dismissed);
    }

    fn previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.saturating_sub(1), cx);
    }

    fn next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected + 1, cx);
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let meta = type_scale(cx).meta;
        let rows = self.themes.iter().enumerate().map(|(ix, base)| {
            let selected = ix == self.selected;
            let mode = match base.appearance {
                Appearance::Dark => "Dark",
                Appearance::Light => "Light",
            };
            let note = match (base.name == self.in_use, base.builtin) {
                (true, _) => format!("{mode} · in use"),
                (false, true) => format!("{mode} · built in"),
                (false, false) => mode.to_owned(),
            };
            list_row(("new-theme-base", ix), selected, cx)
                .mx(px(0.))
                .h(px(28.))
                .child(
                    icon(match base.appearance {
                        Appearance::Dark => IconName::Moon,
                        Appearance::Light => IconName::Sun,
                    })
                    .size(px(14.))
                    .color(if selected { c.accent } else { c.text_muted }),
                )
                .child(div().flex_1().min_w_0().truncate().child(base.name.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_size(meta)
                        .text_color(c.text_muted)
                        .child(note),
                )
                .on_click(cx.listener(move |dialog, _, _, cx| dialog.select(ix, cx)))
        });
        div()
            .id("new-theme-bases")
            .track_scroll(&self.scroll)
            .max_h(px(LIST_H))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
    }
}

/// A field label: meta size, muted, 16 px from the edges.
fn label(text: &'static str, cx: &App) -> gpui::Div {
    div()
        .px(px(16.))
        .pt(px(10.))
        .pb(px(6.))
        .text_size(type_scale(cx).meta)
        .text_color(theme(cx).colors.text_muted)
        .child(text)
}

impl Render for NewThemeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let focused = self.input.focus_handle(cx).is_focused(window);
        let buttons = [
            button("new-theme-cancel", "Cancel")
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(|d, _, window, cx| d.cancel(&Cancel, window, cx)))
                .into_any_element(),
            button("new-theme-create", "Create")
                .style(ButtonStyle::Primary)
                .on_click(cx.listener(|d, _, window, cx| d.create(&Create, window, cx)))
                .into_any_element(),
        ];
        let hint = div()
            .when(self.error, |el| el.text_color(c.error))
            .child(self.hint.clone())
            .into_any_element();
        dialog_frame(px(460.), cx)
            .id("new-theme-dialog")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::create))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::next))
            .child(dialog_title("New theme", cx))
            .child(label("Name", cx).pt(px(0.)))
            .child(
                div().px(px(16.)).child(
                    input_frame(focused, true, cx)
                        .child(icon(IconName::Palette).size(px(15.)).color(c.accent))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .overflow_hidden()
                                .child(self.input.clone()),
                        ),
                ),
            )
            .child(label("Start from", cx))
            .child(div().px(px(12.)).child(self.render_list(cx)))
            .child(dialog_buttons(Some(hint), buttons, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_names_are_free() {
        let mut registry = ThemeRegistry::builtin();
        assert_eq!(suggested_name("Arcode Ember", &registry), "My Ember");
        let mine = |name: &str| arcode_config::Theme {
            name: name.into(),
            ..ThemeRegistry::builtin()
                .get("Arcode Ember")
                .unwrap()
                .clone()
        };
        registry.insert(mine("My Ember"));
        registry.insert(mine("My Ember 2"));
        assert_eq!(suggested_name("Arcode Ember", &registry), "My Ember 3");
        assert_eq!(suggested_name("Night", &registry), "Night copy");
    }

    #[test]
    fn every_theme_is_listed_with_its_mode() {
        let mut registry = ThemeRegistry::builtin();
        registry.insert(arcode_config::Theme {
            name: "Paper".into(),
            ..ThemeRegistry::builtin()
                .get("Arcode Chalk")
                .unwrap()
                .clone()
        });
        let bases = base_themes(&registry);
        assert_eq!(bases.len(), registry.names().len());
        assert!(bases[0].builtin);
        let paper = bases.iter().find(|b| b.name == "Paper").unwrap();
        assert!(!paper.builtin);
        assert_eq!(paper.appearance, Appearance::Light);
    }
}
