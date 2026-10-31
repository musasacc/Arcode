//! The Settings page: a tab with sections (Appearance, Editor, Fonts,
//! Terminal, Languages, Keymap, Plugins, Advanced). Every control edits
//! `config.toml` in place through [`crate::prefs::store`], so comments and
//! hand-written settings survive; the page redraws when the settings
//! change, whatever changed them.

mod appearance;
mod editor;
mod filter;
mod fonts;
mod keymap;
mod languages;
mod nav;
mod other;
mod plugins;
mod rows;
#[cfg(test)]
mod tests;
pub mod theme_card;
pub mod widgets;

pub(crate) use rows::{Steps, keymap_index};

use arcode_config::SettingEdit;
use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{IconName, theme};
use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle,
    SharedString, Subscription, WeakEntity, Window, canvas, div, prelude::*, px,
};

use crate::prefs::pickers::{FontTarget, ThemeSlot};
use crate::text_field::{TextChanged, TextField};
use crate::workspace::Workspace;

/// Key of the page tab.
pub const PAGE_KEY: &str = "settings";

/// Below this page width the section nav shows only its icons.
const COMPACT_WIDTH: f32 = 560.;

/// A section of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Editor,
    Fonts,
    Terminal,
    Languages,
    Keymap,
    Plugins,
    Advanced,
}

impl Section {
    /// Every section in display order.
    pub const ALL: [Section; 8] = [
        Self::Appearance,
        Self::Editor,
        Self::Fonts,
        Self::Terminal,
        Self::Languages,
        Self::Keymap,
        Self::Plugins,
        Self::Advanced,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Editor => "Editor",
            Self::Fonts => "Fonts",
            Self::Terminal => "Terminal",
            Self::Languages => "Languages",
            Self::Keymap => "Keymap",
            Self::Plugins => "Plugins",
            Self::Advanced => "Advanced",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Editor => IconName::FileText,
            Self::Fonts => IconName::Type,
            Self::Terminal => IconName::Terminal,
            Self::Languages => IconName::Puzzle,
            Self::Keymap => IconName::Keyboard,
            Self::Plugins => IconName::Package,
            Self::Advanced => IconName::Settings,
        }
    }
}

/// Requests the page passes to the workspace.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEvent {
    /// Open the theme picker for a slot.
    PickTheme(ThemeSlot),
    /// Open the font picker for a setting.
    PickFont(FontTarget),
    /// Open `config.toml` in an editor.
    OpenFile,
    /// Start the tour.
    StartTour,
    /// Show the first-run dialog.
    ShowWelcome,
}

/// Writes settings from click handlers, reporting failures as toasts.
#[derive(Clone)]
pub struct Writer(WeakEntity<Workspace>);

impl Writer {
    /// Writes `edits` to `config.toml`.
    pub fn write(&self, edits: Vec<SettingEdit>, cx: &mut App) {
        crate::prefs::store::write_and_report(edits, self.0.clone(), cx);
    }

    /// Writes one value.
    pub fn set(&self, path: &str, value: impl Into<arcode_config::SettingValue>, cx: &mut App) {
        self.write(vec![SettingEdit::set(path, value)], cx);
    }
}

/// The page view.
pub struct SettingsPage {
    focus: FocusHandle,
    section: Section,
    writer: Writer,
    /// Filter of the Keymap section.
    keymap_filter: Entity<TextField>,
    /// "Filter settings": rows of every section that match.
    filter: Entity<TextField>,
    scroll: ScrollHandle,
    /// The theme card whose theme is previewed (hovered).
    hover_preview: Option<SharedString>,
    /// The page is narrow (a split pane): icon-only nav. Measured while
    /// drawing; a change redraws the page once.
    compact: bool,
    measured_compact: Rc<Cell<bool>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsPage {}

impl Focusable for SettingsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsPage {
    /// A page that reports write failures to `workspace`.
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let keymap_filter = cx.new(|cx| TextField::new("Filter commands or keys", cx));
        let filter = cx.new(|cx| TextField::new("Filter settings", cx));
        let subs = vec![
            cx.observe_global::<arcode_ui_kit::Settings>(|_, cx| cx.notify()),
            cx.subscribe(&keymap_filter, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&filter, |page, _, _: &TextChanged, cx| {
                page.scroll.set_offset(gpui::point(px(0.), px(0.)));
                cx.notify()
            }),
        ];
        Self {
            focus: cx.focus_handle(),
            section: Section::Appearance,
            writer: Writer(workspace),
            keymap_filter,
            filter,
            scroll: ScrollHandle::new(),
            hover_preview: None,
            compact: false,
            measured_compact: Rc::default(),
            _subs: subs,
        }
    }

    /// The shown section.
    pub fn section(&self) -> Section {
        self.section
    }

    /// Shows `section`.
    pub fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// A theme card was hovered or left. Only the card that started the
    /// preview ends it: moving straight to the next card hovers it before
    /// the previous one reports leaving.
    pub(crate) fn hover_theme(
        &mut self,
        card: &SharedString,
        preview: Option<&arcode_config::Theme>,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if hovered {
            self.hover_preview = Some(card.clone());
            match preview {
                Some(theme) => crate::theme_sync::preview(theme, cx),
                None => crate::theme_sync::apply(window.appearance(), cx),
            }
        } else if self.hover_preview.as_ref() == Some(card) {
            self.end_hover_preview(window, cx);
        }
    }

    /// Ends a hover preview (the page was hidden while a card was hovered).
    pub fn end_hover_preview(&mut self, window: &mut Window, cx: &mut App) {
        if self.hover_preview.take().is_some() {
            crate::theme_sync::apply(window.appearance(), cx);
        }
    }

    /// Whether a theme card's preview is shown.
    pub fn hover_previewing(&self) -> bool {
        self.hover_preview.is_some()
    }

    /// The writer handlers use.
    pub fn writer(&self) -> Writer {
        self.writer.clone()
    }

    /// Whether the page filter holds text (every section's matching rows
    /// are shown).
    pub fn filtering(&self, cx: &App) -> bool {
        !filter::words(self.filter.read(cx).text()).is_empty()
    }

    /// Empties the page filter.
    pub fn clear_filter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.filter.read(cx).text().is_empty() {
            self.filter.update(cx, |f, cx| f.set_text("", cx));
        }
    }

    /// Records whether the page is narrow; redraws the page when that
    /// changed (only then, so an idle page draws nothing).
    fn measure_width(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.measured_compact.clone();
        let shown = self.compact;
        let page = cx.entity_id();
        canvas(
            move |bounds, _, cx| {
                let compact = bounds.size.width < px(COMPACT_WIDTH);
                measured.set(compact);
                if compact != shown {
                    // Not while drawing: a notification then is dropped.
                    cx.defer(move |cx| cx.notify(page));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Whether the page shows its narrow layout.
    pub fn is_compact(&self) -> bool {
        self.compact
    }

    fn render_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        self.render_section_of(self.section, cx)
    }

    fn render_section_of(&self, section: Section, cx: &mut Context<Self>) -> gpui::Div {
        match section {
            Section::Appearance => appearance::render(self, cx),
            Section::Editor => editor::render(self, cx),
            Section::Fonts => fonts::render(self, cx),
            Section::Terminal => other::render_terminal(self, cx),
            Section::Languages => languages::render(self, cx),
            Section::Keymap => keymap::render(self, &self.keymap_filter, cx),
            Section::Plugins => plugins::render(self, cx),
            Section::Advanced => other::render_advanced(self, cx),
        }
    }
}

/// The heading above a group of rows: title size SemiBold, sentence case.
/// Hidden while the page filter shows matching rows only.
pub(crate) fn group_title(text: &'static str, cx: &App) -> gpui::Div {
    if filter::active() {
        return div();
    }
    div()
        .pt(px(22.))
        .pb(px(2.))
        .text_size(type_scale(cx).title)
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(theme(cx).colors.text)
        .child(text)
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.compact = self.measured_compact.get();
        let c = theme(cx).colors.clone();
        let header = self.render_header(window, cx);
        let body = if self.filtering(cx) {
            self.render_filtered(cx)
        } else {
            self.render_section(cx)
        };
        div()
            .id("settings-page")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(c.panel())
            .text_color(c.text)
            .child(self.measure_width(cx))
            .child(self.render_nav(cx))
            .child(
                div()
                    .id("settings-scroll")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .max_w(px(812.))
                            .px(px(if self.compact { 12. } else { 36. }))
                            .pt(px(22.))
                            .pb(px(36.))
                            .child(header)
                            .child(body),
                    ),
            )
    }
}
